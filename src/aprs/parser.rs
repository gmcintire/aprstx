use super::packet::{AprsPacket, CallSign};
use anyhow::{anyhow, Result};
use chrono::Utc;

pub fn parse_packet(input: &str) -> Result<AprsPacket> {
    // Only trim leading whitespace to preserve trailing spaces in the information field
    let input = input.trim_start();
    if input.is_empty() {
        return Err(anyhow!("Empty packet"));
    }

    let header_end = input
        .find(':')
        .ok_or_else(|| anyhow!("No ':' separator found"))?;
    let (header, information) = input.split_at(header_end);
    let header = header.trim(); // Trim the header part
    let information = &information[1..];

    let header_parts: Vec<&str> = header.split('>').collect();
    if header_parts.len() != 2 {
        return Err(anyhow!("Invalid header format"));
    }

    let source =
        CallSign::parse(header_parts[0]).ok_or_else(|| anyhow!("Invalid source callsign"))?;

    let path_parts: Vec<&str> = header_parts[1].split(',').collect();
    if path_parts.is_empty() {
        return Err(anyhow!("No destination in header"));
    }

    let destination =
        CallSign::parse(path_parts[0]).ok_or_else(|| anyhow!("Invalid destination callsign"))?;

    let mut path = Vec::new();
    for path_part in path_parts.iter().skip(1) {
        if let Some(call) = CallSign::parse(path_part) {
            path.push(call);
        }
    }

    let mut packet = AprsPacket::new(source, destination, information.to_string());
    packet.path = path;
    packet.timestamp = Utc::now();

    Ok(packet)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic_packet() {
        let input = "N0CALL>APRS:>Test status";
        let packet = parse_packet(input).unwrap();

        assert_eq!(packet.source.call, "N0CALL");
        assert_eq!(packet.destination.call, "APRS");
        assert_eq!(packet.information, ">Test status");
    }

    #[test]
    fn test_parse_packet_with_path() {
        let input = "N0CALL-5>APRS,WIDE1-1,WIDE2-2:!4903.50N/07201.75W>Test";
        let packet = parse_packet(input).unwrap();

        assert_eq!(packet.source.call, "N0CALL");
        assert_eq!(packet.source.ssid.0, 5);
        assert_eq!(packet.destination.call, "APRS");
        assert_eq!(packet.path.len(), 2);
        assert_eq!(packet.path[0].to_string(), "WIDE1-1");
        assert_eq!(packet.path[1].to_string(), "WIDE2-2");
    }

    #[test]
    fn test_parse_packet_with_spaces() {
        let input = " N0CALL>APRS:>Test status ";
        let packet = parse_packet(input).unwrap();
        assert_eq!(packet.source.call, "N0CALL");
        assert_eq!(packet.information, ">Test status ");
    }

    #[test]
    fn test_parse_errors() {
        // Empty packet
        assert!(parse_packet("").is_err());

        // No separator
        assert!(parse_packet("N0CALL>APRS").is_err());

        // No destination
        assert!(parse_packet("N0CALL>:test").is_err());

        // No source
        assert!(parse_packet(">APRS:test").is_err());

        // Invalid format
        assert!(parse_packet("invalid packet").is_err());
    }

    #[test]
    fn test_parse_digipeated_packet() {
        let input = "N0CALL>APRS,WIDE1-1*,WIDE2-2:>Test";
        let packet = parse_packet(input).unwrap();

        assert_eq!(packet.path.len(), 2);
        assert_eq!(packet.path[0].call, "WIDE1");
        assert_eq!(packet.path[0].ssid.0, 1);
        assert!(packet.path[0].digipeated);
        assert_eq!(packet.path[1].call, "WIDE2");
        assert_eq!(packet.path[1].ssid.0, 2);
        assert!(!packet.path[1].digipeated);
    }

    #[test]
    fn test_parse_long_path() {
        let input = "N0CALL>APRS,A,B,C,D,E,F,G,H:>Test";
        let packet = parse_packet(input).unwrap();

        assert_eq!(packet.path.len(), 8);
        assert_eq!(packet.path[0].call, "A");
        assert_eq!(packet.path[7].call, "H");
    }

    #[test]
    fn test_parse_special_characters() {
        let input = "N0CALL>APRS::N1CALL   :Test message{123";
        let packet = parse_packet(input).unwrap();

        assert_eq!(packet.information, ":N1CALL   :Test message{123");
    }
}
