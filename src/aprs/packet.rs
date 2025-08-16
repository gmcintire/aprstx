use chrono::{DateTime, Utc};
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct AprsPacket {
    pub source: CallSign,
    pub destination: CallSign,
    pub path: Vec<CallSign>,
    pub data_type: DataType,
    pub information: String,
    pub timestamp: DateTime<Utc>,
    pub raw: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CallSign {
    pub call: String,
    pub ssid: Ssid,
    pub digipeated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ssid(pub u8);

#[derive(Debug, Clone, PartialEq)]
pub enum DataType {
    Position,
    Status,
    Message,
    Object,
    Item,
    MicE,
    Telemetry,
    Weather,
    UserDefined,
    ThirdParty,
    Invalid,
}

impl CallSign {
    pub fn new(call: &str, ssid: u8) -> Self {
        CallSign {
            call: call.to_uppercase(),
            ssid: Ssid(ssid),
            digipeated: false,
        }
    }

    pub fn parse(input: &str) -> Option<Self> {
        // Check for digipeated marker
        let (input, digipeated) = if let Some(stripped) = input.strip_suffix('*') {
            (stripped, true)
        } else {
            (input, false)
        };

        let parts: Vec<&str> = input.split('-').collect();
        if parts.is_empty() || parts[0].is_empty() {
            return None;
        }

        let call = parts[0].to_uppercase();
        let ssid = if parts.len() > 1 {
            parts[1].parse::<u8>().unwrap_or(0)
        } else {
            0
        };

        if ssid > 15 {
            return None;
        }

        Some(CallSign {
            call,
            ssid: Ssid(ssid),
            digipeated,
        })
    }
}

impl fmt::Display for CallSign {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ssid.0 == 0 {
            write!(f, "{}{}", self.call, if self.digipeated { "*" } else { "" })
        } else {
            write!(
                f,
                "{}-{}{}",
                self.call,
                self.ssid.0,
                if self.digipeated { "*" } else { "" }
            )
        }
    }
}

impl AprsPacket {
    pub fn new(source: CallSign, destination: CallSign, information: String) -> Self {
        let data_type = Self::detect_data_type(&information);

        AprsPacket {
            source,
            destination,
            path: Vec::new(),
            data_type,
            information,
            timestamp: Utc::now(),
            raw: None,
        }
    }

    fn detect_data_type(info: &str) -> DataType {
        if info.is_empty() {
            return DataType::Invalid;
        }

        match info.chars().next().unwrap() {
            '!' | '=' => DataType::Position,
            '/' | '@' => DataType::Position,
            '>' => DataType::Status,
            ':' => DataType::Message,
            ';' => DataType::Object,
            ')' => DataType::Item,
            '`' | '\'' => DataType::MicE,
            'T' => DataType::Telemetry,
            '_' => DataType::Weather,
            '{' => DataType::UserDefined,
            '}' => DataType::ThirdParty,
            _ => DataType::Invalid,
        }
    }

    pub fn has_rfonly(&self) -> bool {
        self.information.contains("RFONLY")
    }

    pub fn has_nogate(&self) -> bool {
        self.information.contains("NOGATE")
    }
}

impl fmt::Display for AprsPacket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}>", self.source)?;
        write!(f, "{}", self.destination)?;

        for hop in &self.path {
            write!(f, ",{}", hop)?;
        }

        write!(f, ":{}", self.information)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_callsign_new() {
        let call = CallSign::new("n0call", 5);
        assert_eq!(call.call, "N0CALL");
        assert_eq!(call.ssid.0, 5);
    }

    #[test]
    fn test_callsign_parse() {
        // Basic callsign
        let call = CallSign::parse("N0CALL").unwrap();
        assert_eq!(call.call, "N0CALL");
        assert_eq!(call.ssid.0, 0);

        // Callsign with SSID
        let call = CallSign::parse("N0CALL-5").unwrap();
        assert_eq!(call.call, "N0CALL");
        assert_eq!(call.ssid.0, 5);

        // Invalid SSID
        assert!(CallSign::parse("N0CALL-16").is_none());

        // Empty string
        assert!(CallSign::parse("").is_none());

        // Just dash
        assert!(CallSign::parse("-5").is_none());
    }

    #[test]
    fn test_callsign_display() {
        let call = CallSign::new("N0CALL", 0);
        assert_eq!(call.to_string(), "N0CALL");

        let call = CallSign::new("N0CALL", 5);
        assert_eq!(call.to_string(), "N0CALL-5");
    }

    #[test]
    fn test_data_type_detection() {
        assert_eq!(
            AprsPacket::detect_data_type("!4903.50N/07201.75W>"),
            DataType::Position
        );
        assert_eq!(
            AprsPacket::detect_data_type("=4903.50N/07201.75W>"),
            DataType::Position
        );
        assert_eq!(
            AprsPacket::detect_data_type("/4903.50N/07201.75W>"),
            DataType::Position
        );
        assert_eq!(
            AprsPacket::detect_data_type("@4903.50N/07201.75W>"),
            DataType::Position
        );
        assert_eq!(
            AprsPacket::detect_data_type(">Status text"),
            DataType::Status
        );
        assert_eq!(
            AprsPacket::detect_data_type(":N0CALL   :Hello"),
            DataType::Message
        );
        assert_eq!(
            AprsPacket::detect_data_type(";Object   *"),
            DataType::Object
        );
        assert_eq!(AprsPacket::detect_data_type(")Item!"), DataType::Item);
        assert_eq!(AprsPacket::detect_data_type("`MicE data"), DataType::MicE);
        assert_eq!(AprsPacket::detect_data_type("'MicE data"), DataType::MicE);
        assert_eq!(
            AprsPacket::detect_data_type("T#001,123,456"),
            DataType::Telemetry
        );
        assert_eq!(AprsPacket::detect_data_type("_weather"), DataType::Weather);
        assert_eq!(AprsPacket::detect_data_type("{user"), DataType::UserDefined);
        assert_eq!(AprsPacket::detect_data_type("}third"), DataType::ThirdParty);
        assert_eq!(AprsPacket::detect_data_type(""), DataType::Invalid);
        assert_eq!(AprsPacket::detect_data_type("Invalid"), DataType::Invalid);
    }

    #[test]
    fn test_packet_creation() {
        let source = CallSign::new("N0CALL", 5);
        let dest = CallSign::new("APRS", 0);
        let packet = AprsPacket::new(source, dest, ">Test status".to_string());

        assert_eq!(packet.source.call, "N0CALL");
        assert_eq!(packet.source.ssid.0, 5);
        assert_eq!(packet.destination.call, "APRS");
        assert_eq!(packet.data_type, DataType::Status);
        assert_eq!(packet.information, ">Test status");
    }

    #[test]
    fn test_packet_display() {
        let source = CallSign::new("N0CALL", 5);
        let dest = CallSign::new("APRS", 0);
        let mut packet = AprsPacket::new(source, dest, ">Test status".to_string());

        // Without path
        assert_eq!(packet.to_string(), "N0CALL-5>APRS:>Test status");

        // With path
        packet.path.push(CallSign::new("WIDE1", 1));
        packet.path.push(CallSign::new("WIDE2", 2));
        assert_eq!(
            packet.to_string(),
            "N0CALL-5>APRS,WIDE1-1,WIDE2-2:>Test status"
        );
    }

    #[test]
    fn test_rfonly_nogate() {
        let source = CallSign::new("N0CALL", 0);
        let dest = CallSign::new("APRS", 0);

        let packet = AprsPacket::new(source.clone(), dest.clone(), ">Test RFONLY".to_string());
        assert!(packet.has_rfonly());
        assert!(!packet.has_nogate());

        let packet = AprsPacket::new(source.clone(), dest.clone(), ">Test NOGATE".to_string());
        assert!(!packet.has_rfonly());
        assert!(packet.has_nogate());

        let packet = AprsPacket::new(source, dest, ">Test normal".to_string());
        assert!(!packet.has_rfonly());
        assert!(!packet.has_nogate());
    }
}
