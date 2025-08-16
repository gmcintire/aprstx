use crate::aprs::parse_packet;
use crate::config::AprsIsConfig;
use crate::router::{PacketSource, RoutedPacket};
use anyhow::{anyhow, Result};
use log::{debug, error, info, warn};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{broadcast, mpsc};
use tokio::time::{interval, timeout};

const APRS_IS_TIMEOUT: Duration = Duration::from_secs(30);
const APRS_IS_KEEPALIVE: Duration = Duration::from_secs(20);

pub async fn run_aprs_is_connection(
    config: AprsIsConfig,
    packet_tx: mpsc::Sender<RoutedPacket>,
    is_rx: broadcast::Receiver<RoutedPacket>,
) -> Result<()> {
    loop {
        match connect_and_run(&config, packet_tx.clone(), is_rx.resubscribe()).await {
            Ok(_) => {
                warn!("APRS-IS connection closed normally, reconnecting in 30s...");
            }
            Err(e) => {
                error!("APRS-IS connection error: {}, reconnecting in 30s...", e);
            }
        }
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}

async fn connect_and_run(
    config: &AprsIsConfig,
    packet_tx: mpsc::Sender<RoutedPacket>,
    mut is_rx: broadcast::Receiver<RoutedPacket>,
) -> Result<()> {
    info!(
        "Connecting to APRS-IS server {}:{}",
        config.server, config.port
    );

    let stream = timeout(
        APRS_IS_TIMEOUT,
        TcpStream::connect(format!("{}:{}", config.server, config.port)),
    )
    .await??;

    info!("Connected to APRS-IS server");

    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    reader.read_line(&mut line).await?;
    info!("APRS-IS server banner: {}", line.trim());
    line.clear();

    let passcode = if config.passcode == "-1" {
        -1
    } else {
        config
            .passcode
            .parse()
            .unwrap_or_else(|_| calculate_passcode(&config.callsign))
    };

    let login = format!(
        "user {} pass {} vers aprstx 0.1.0{}\r\n",
        config.callsign,
        passcode,
        config
            .filter
            .as_ref()
            .map(|f| format!(" filter {}", f))
            .unwrap_or_default()
    );

    writer.write_all(login.as_bytes()).await?;
    info!("Sent login to APRS-IS");

    reader.read_line(&mut line).await?;
    if !line.contains("verified") && !line.contains("unverified") {
        return Err(anyhow!("APRS-IS login failed: {}", line.trim()));
    }
    info!("APRS-IS login successful: {}", line.trim());
    line.clear();

    let mut keepalive_timer = interval(APRS_IS_KEEPALIVE);

    loop {
        tokio::select! {
            result = reader.read_line(&mut line) => {
                match result {
                    Ok(0) => {
                        info!("APRS-IS connection closed by server");
                        break;
                    }
                    Ok(_) => {
                        let trimmed = line.trim();
                        if trimmed.starts_with('#') {
                            debug!("APRS-IS server message: {}", trimmed);
                        } else if !trimmed.is_empty() {
                            if let Ok(packet) = parse_packet(trimmed) {
                                info!("RX [APRS-IS]: {}", packet);

                                if config.rx_enable {
                                    let routed = RoutedPacket {
                                        packet,
                                        source: PacketSource::AprsIs,
                                    };
                                    let _ = packet_tx.send(routed).await;
                                }
                            }
                        }
                        line.clear();
                    }
                    Err(e) => {
                        error!("APRS-IS read error: {}", e);
                        break;
                    }
                }
            }

            Ok(routed) = is_rx.recv() => {
                if config.tx_enable {
                    let aprs_line = format!("{}\r\n", routed.packet);
                    if let Err(e) = writer.write_all(aprs_line.as_bytes()).await {
                        error!("Failed to send to APRS-IS: {}", e);
                        break;
                    } else {
                        info!("TX [APRS-IS]: {}", routed.packet);
                    }
                }
            }

            _ = keepalive_timer.tick() => {
                debug!("Sending APRS-IS keepalive");
                if let Err(e) = writer.write_all(b"# keepalive\r\n").await {
                    error!("Failed to send keepalive: {}", e);
                    break;
                }
            }
        }
    }

    Ok(())
}

fn calculate_passcode(callsign: &str) -> i32 {
    let call_upper = callsign.split('-').next().unwrap_or("").to_uppercase();
    let mut hash: i32 = 0x73e2;

    for (i, ch) in call_upper.chars().enumerate() {
        if i % 2 == 0 {
            hash ^= (ch as i32) << 8;
        } else {
            hash ^= ch as i32;
        }
    }

    hash & 0x7fff
}
