mod kiss;
pub mod pure_serial;

use crate::aprs::{parse_packet, AprsPacket};
use crate::config::{SerialPortConfig, SerialProtocol};
use crate::router::{PacketSource, RoutedPacket};
use anyhow::{anyhow, Result};
use bytes::BytesMut;
use kiss::KissCodec;
use log::{debug, error, info};
use pure_serial::SerialPort;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{broadcast, mpsc};

pub async fn run_serial_port(
    config: SerialPortConfig,
    packet_tx: mpsc::Sender<RoutedPacket>,
    rf_rx: broadcast::Receiver<RoutedPacket>,
) -> Result<()> {
    info!("Opening serial port {} on {}", config.name, config.device);

    let port = SerialPort::open(&config.device, config.baud_rate).await?;

    info!("Serial port {} opened successfully", config.name);

    match config.protocol {
        SerialProtocol::Kiss => run_kiss_protocol(config, port, packet_tx, rf_rx).await,
        SerialProtocol::Tnc2 => run_tnc2_protocol(config, port, packet_tx, rf_rx).await,
    }
}

async fn run_kiss_protocol(
    config: SerialPortConfig,
    mut port: SerialPort,
    packet_tx: mpsc::Sender<RoutedPacket>,
    mut rf_rx: broadcast::Receiver<RoutedPacket>,
) -> Result<()> {
    let mut codec = KissCodec::new();
    let mut read_buf = BytesMut::with_capacity(1024);
    let mut temp_buf = [0u8; 256];

    loop {
        tokio::select! {
            // Handle incoming data from serial port
            result = port.read(&mut temp_buf) => {
                match result {
                    Ok(n) if n > 0 => {
                        read_buf.extend_from_slice(&temp_buf[..n]);

                        while let Some(frame) = codec.decode(&mut read_buf)? {
                            debug!("Received KISS frame: {} bytes", frame.len());

                            if let Ok(ax25_frame) = ax25_to_aprs(&frame) {
                                if let Ok(packet) = parse_packet(&ax25_frame) {
                                    info!("RX [{}]: {}", config.name, packet);

                                    if config.rx_enable {
                                        let routed = RoutedPacket {
                                            packet,
                                            source: PacketSource::SerialPort(config.name.clone()),
                                        };
                                        let _ = packet_tx.send(routed).await;
                                    }
                                }
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                    Err(e) => {
                        error!("Serial port read error: {}", e);
                        return Err(e.into());
                    }
                }
            }

            // Handle packets to transmit
            Ok(routed) = rf_rx.recv() => {
                if config.tx_enable {
                    if let Ok(ax25_frame) = aprs_to_ax25(&routed.packet) {
                        let kiss_frame = codec.encode(&ax25_frame, 0);
                        if let Err(e) = port.write_all(&kiss_frame).await {
                            error!("Failed to write to serial port: {}", e);
                        } else {
                            info!("TX [{}]: {}", config.name, routed.packet);
                        }
                    }
                }
            }
        }
    }
}

async fn run_tnc2_protocol(
    config: SerialPortConfig,
    mut port: SerialPort,
    packet_tx: mpsc::Sender<RoutedPacket>,
    mut rf_rx: broadcast::Receiver<RoutedPacket>,
) -> Result<()> {
    let mut line_buffer = String::new();
    let mut temp_buf = [0u8; 256];

    loop {
        tokio::select! {
            // Handle incoming data from serial port
            result = port.read(&mut temp_buf) => {
                match result {
                    Ok(n) if n > 0 => {
                        let text = String::from_utf8_lossy(&temp_buf[..n]);
                        line_buffer.push_str(&text);

                        while let Some(pos) = line_buffer.find('\n') {
                            let line = line_buffer[..pos].trim_end_matches('\r');

                            if !line.is_empty() {
                                if let Ok(packet) = parse_packet(line) {
                                    info!("RX [{}]: {}", config.name, packet);

                                    if config.rx_enable {
                                        let routed = RoutedPacket {
                                            packet,
                                            source: PacketSource::SerialPort(config.name.clone()),
                                        };
                                        let _ = packet_tx.send(routed).await;
                                    }
                                }
                            }

                            line_buffer.drain(..=pos);
                        }
                    }
                    Ok(_) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                    Err(e) => {
                        error!("Serial port read error: {}", e);
                        return Err(e.into());
                    }
                }
            }

            // Handle packets to transmit
            Ok(routed) = rf_rx.recv() => {
                if config.tx_enable {
                    let tnc2_frame = format!("{}\r\n", routed.packet);
                    if let Err(e) = port.write_all(tnc2_frame.as_bytes()).await {
                        error!("Failed to write to serial port: {}", e);
                    } else {
                        info!("TX [{}]: {}", config.name, routed.packet);
                    }
                }
            }
        }
    }
}

fn ax25_to_aprs(frame: &[u8]) -> Result<String> {
    if frame.len() < 16 {
        return Err(anyhow!("Frame too short"));
    }

    let mut i = 0;

    // Decode destination
    let dest = decode_ax25_address(&frame[i..i + 7])?;
    i += 7;

    // Decode source
    let src = decode_ax25_address(&frame[i..i + 7])?;
    i += 7;
    let last_bit = frame[i - 1] & 0x01;

    // Start building result: source>dest
    let mut result = format!("{}>{}", src, dest);

    // Decode digipeater path if present
    if last_bit == 0 {
        while i < frame.len() && (frame[i - 1] & 0x01) == 0 {
            if i + 7 > frame.len() {
                break;
            }
            result.push(',');
            let digi = decode_ax25_address(&frame[i..i + 7])?;
            result.push_str(&digi);
            i += 7;
        }
    }

    // Check for control and PID fields
    if i + 2 <= frame.len() && frame[i] == 0x03 && frame[i + 1] == 0xF0 {
        i += 2;
        result.push(':');
        result.push_str(&String::from_utf8_lossy(&frame[i..]));
    }

    Ok(result)
}

fn decode_ax25_address(data: &[u8]) -> Result<String> {
    if data.len() < 7 {
        return Err(anyhow!("Invalid AX.25 address"));
    }

    let mut call = String::new();
    for &byte in data.iter().take(6) {
        let c = (byte >> 1) as char;
        if c != ' ' {
            call.push(c);
        }
    }

    let ssid = (data[6] >> 1) & 0x0F;
    if ssid > 0 {
        call.push_str(&format!("-{}", ssid));
    }

    Ok(call)
}

fn aprs_to_ax25(packet: &AprsPacket) -> Result<Vec<u8>> {
    let mut frame = Vec::new();

    // Encode destination
    encode_ax25_address(&packet.destination, false, &mut frame)?;

    // Encode source
    let last_addr = packet.path.is_empty();
    encode_ax25_address(&packet.source, last_addr, &mut frame)?;

    // Encode path
    for (i, hop) in packet.path.iter().enumerate() {
        let last = i == packet.path.len() - 1;
        encode_ax25_address(hop, last, &mut frame)?;
    }

    // Add control and PID
    frame.push(0x03); // UI frame
    frame.push(0xF0); // No layer 3 protocol

    // Add information field
    frame.extend_from_slice(packet.information.as_bytes());

    Ok(frame)
}

fn encode_ax25_address(
    call: &crate::aprs::CallSign,
    last: bool,
    frame: &mut Vec<u8>,
) -> Result<()> {
    let mut addr = [0x20u8 << 1; 7]; // Space-filled (0x20 shifted left = 0x40)

    // Encode callsign
    let call_bytes = call.call.as_bytes();
    for (i, &b) in call_bytes.iter().take(6).enumerate() {
        addr[i] = b << 1;
    }

    // Encode SSID
    addr[6] = (call.ssid.0 << 1) | 0x60;

    // Set end-of-address bit if this is the last address
    if last {
        addr[6] |= 0x01;
    }

    frame.extend_from_slice(&addr);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aprs::CallSign;

    #[test]
    fn test_decode_ax25_address() {
        // Simple callsign
        let data = [0x9C, 0x60, 0x86, 0x82, 0x98, 0x98, 0x60]; // N0CALL
        let result = decode_ax25_address(&data).unwrap();
        assert_eq!(result, "N0CALL");

        // Callsign with SSID
        let data = [0x9C, 0x60, 0x86, 0x82, 0x98, 0x98, 0x6A]; // N0CALL-5
        let result = decode_ax25_address(&data).unwrap();
        assert_eq!(result, "N0CALL-5");

        // Short callsign
        let data = [0x82, 0x84, 0x86, 0x40, 0x40, 0x40, 0x60]; // ABC (A=0x41<<1=0x82, B=0x42<<1=0x84, C=0x43<<1=0x86)
        let result = decode_ax25_address(&data).unwrap();
        assert_eq!(result, "ABC");

        // Invalid length
        assert!(decode_ax25_address(&[0x00; 6]).is_err());
    }

    #[test]
    fn test_encode_ax25_address() {
        let mut frame = Vec::new();

        // Simple callsign
        let call = CallSign::new("N0CALL", 0);
        encode_ax25_address(&call, false, &mut frame).unwrap();
        assert_eq!(frame, vec![0x9C, 0x60, 0x86, 0x82, 0x98, 0x98, 0x60]);

        // With SSID and last address bit
        frame.clear();
        let call = CallSign::new("N0CALL", 5);
        encode_ax25_address(&call, true, &mut frame).unwrap();
        assert_eq!(frame, vec![0x9C, 0x60, 0x86, 0x82, 0x98, 0x98, 0x6B]);

        // Short callsign
        frame.clear();
        let call = CallSign::new("ABC", 0);
        encode_ax25_address(&call, false, &mut frame).unwrap();
        assert_eq!(frame, vec![0x82, 0x84, 0x86, 0x40, 0x40, 0x40, 0x60]);
    }

    #[test]
    fn test_ax25_to_aprs() {
        // Basic packet
        let frame = vec![
            // Destination: APRS
            0x82,
            0xA0,
            0xA4,
            0xA6,
            0x40,
            0x40,
            0x60,
            // Source: N0CALL-5
            0x9C,
            0x60,
            0x86,
            0x82,
            0x98,
            0x98,
            0x6B,
            // Control, PID
            0x03,
            0xF0,
            // Information
            b'>'.to_owned(),
            b'T'.to_owned(),
            b'e'.to_owned(),
            b's'.to_owned(),
            b't'.to_owned(),
        ];

        let result = ax25_to_aprs(&frame).unwrap();
        assert_eq!(result, "N0CALL-5>APRS:>Test");

        // With digipeater path
        let frame = vec![
            // Destination: APRS
            0x82,
            0xA0,
            0xA4,
            0xA6,
            0x40,
            0x40,
            0x60,
            // Source: TEST
            0xA8,
            0x8A,
            0xA6,
            0xA8,
            0x40,
            0x40,
            0x60,
            // Digipeater: WIDE1-1
            0xAE,
            0x92,
            0x88,
            0x8A,
            0x62,
            0x40,
            0x63,
            // Control, PID
            0x03,
            0xF0,
            // Information
            b'!'.to_owned(),
        ];

        let result = ax25_to_aprs(&frame).unwrap();
        assert_eq!(result, "TEST>APRS,WIDE1-1:!");

        // Too short frame
        assert!(ax25_to_aprs(&[0x00; 10]).is_err());
    }

    #[test]
    fn test_aprs_to_ax25() {
        let packet = AprsPacket::new(
            CallSign::new("N0CALL", 5),
            CallSign::new("APRS", 0),
            ">Test".to_string(),
        );

        let frame = aprs_to_ax25(&packet).unwrap();

        // Check destination
        assert_eq!(&frame[0..7], &[0x82, 0xA0, 0xA4, 0xA6, 0x40, 0x40, 0x60]);
        // Check source with last bit
        assert_eq!(&frame[7..14], &[0x9C, 0x60, 0x86, 0x82, 0x98, 0x98, 0x6B]);
        // Check control and PID
        assert_eq!(&frame[14..16], &[0x03, 0xF0]);
        // Check information
        assert_eq!(&frame[16..], b">Test");
    }

    #[test]
    fn test_aprs_to_ax25_with_path() {
        let mut packet = AprsPacket::new(
            CallSign::new("TEST", 0),
            CallSign::new("APRS", 0),
            "!".to_string(),
        );
        packet.path.push(CallSign::new("WIDE1", 1));
        packet.path.push(CallSign::new("WIDE2", 2));

        let frame = aprs_to_ax25(&packet).unwrap();

        // Check addresses count (dest + src + 2 digis)
        assert!(frame.len() >= 28); // 7*4 addresses

        // Check last address bit is set on last digi
        assert_eq!(frame[27] & 0x01, 0x01);
    }
}
