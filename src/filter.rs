use crate::aprs::AprsPacket;
use crate::config::{FilterAction, FilterConfig};
use regex::Regex;

pub struct PacketFilter {
    filters: Vec<CompiledFilter>,
}

struct CompiledFilter {
    action: FilterAction,
    regex: Regex,
}

impl PacketFilter {
    pub fn new(configs: Vec<FilterConfig>) -> Result<Self, regex::Error> {
        let mut filters = Vec::new();

        for config in configs {
            let regex = Regex::new(&config.pattern)?;
            filters.push(CompiledFilter {
                action: config.action,
                regex,
            });
        }

        Ok(PacketFilter { filters })
    }

    pub fn should_pass(&self, packet: &AprsPacket) -> bool {
        let packet_str = packet.to_string();

        for filter in &self.filters {
            if filter.regex.is_match(&packet_str) {
                match filter.action {
                    FilterAction::Drop => return false,
                    FilterAction::Pass => return true,
                }
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aprs::CallSign;

    #[test]
    fn test_filter_creation() {
        let configs = vec![FilterConfig {
            name: "test".to_string(),
            action: FilterAction::Drop,
            pattern: "RFONLY".to_string(),
        }];

        let filter = PacketFilter::new(configs).unwrap();
        assert_eq!(filter.filters.len(), 1);
    }

    #[test]
    fn test_filter_drop() {
        let configs = vec![FilterConfig {
            name: "rfonly".to_string(),
            action: FilterAction::Drop,
            pattern: "RFONLY".to_string(),
        }];

        let filter = PacketFilter::new(configs).unwrap();

        let packet = AprsPacket::new(
            CallSign::new("N0CALL", 0),
            CallSign::new("APRS", 0),
            ">Test RFONLY packet".to_string(),
        );

        assert!(!filter.should_pass(&packet));

        let packet = AprsPacket::new(
            CallSign::new("N0CALL", 0),
            CallSign::new("APRS", 0),
            ">Normal packet".to_string(),
        );

        assert!(filter.should_pass(&packet));
    }

    #[test]
    fn test_filter_pass() {
        let configs = vec![
            FilterConfig {
                name: "emergency".to_string(),
                action: FilterAction::Pass,
                pattern: "EMERGENCY".to_string(),
            },
            FilterConfig {
                name: "default".to_string(),
                action: FilterAction::Drop,
                pattern: ".*".to_string(),
            },
        ];

        let filter = PacketFilter::new(configs).unwrap();

        let packet = AprsPacket::new(
            CallSign::new("N0CALL", 0),
            CallSign::new("APRS", 0),
            ">EMERGENCY test".to_string(),
        );

        assert!(filter.should_pass(&packet));

        let packet = AprsPacket::new(
            CallSign::new("N0CALL", 0),
            CallSign::new("APRS", 0),
            ">Normal packet".to_string(),
        );

        assert!(!filter.should_pass(&packet));
    }

    #[test]
    fn test_regex_patterns() {
        let configs = vec![FilterConfig {
            name: "callsign".to_string(),
            action: FilterAction::Drop,
            pattern: r"^N0CALL.*".to_string(),
        }];

        let filter = PacketFilter::new(configs).unwrap();

        let packet = AprsPacket::new(
            CallSign::new("N0CALL", 5),
            CallSign::new("APRS", 0),
            ">Test".to_string(),
        );

        assert!(!filter.should_pass(&packet));

        let packet = AprsPacket::new(
            CallSign::new("N1CALL", 0),
            CallSign::new("APRS", 0),
            ">Test".to_string(),
        );

        assert!(filter.should_pass(&packet));
    }

    #[test]
    fn test_multiple_filters() {
        let configs = vec![
            FilterConfig {
                name: "rfonly".to_string(),
                action: FilterAction::Drop,
                pattern: "RFONLY".to_string(),
            },
            FilterConfig {
                name: "nogate".to_string(),
                action: FilterAction::Drop,
                pattern: "NOGATE".to_string(),
            },
            FilterConfig {
                name: "tcpip".to_string(),
                action: FilterAction::Drop,
                pattern: "TCPIP".to_string(),
            },
        ];

        let filter = PacketFilter::new(configs).unwrap();

        // Test each filter
        let packet = AprsPacket::new(
            CallSign::new("N0CALL", 0),
            CallSign::new("APRS", 0),
            ">Test RFONLY".to_string(),
        );
        assert!(!filter.should_pass(&packet));

        let packet = AprsPacket::new(
            CallSign::new("N0CALL", 0),
            CallSign::new("APRS", 0),
            ">Test NOGATE".to_string(),
        );
        assert!(!filter.should_pass(&packet));

        let mut packet = AprsPacket::new(
            CallSign::new("N0CALL", 0),
            CallSign::new("APRS", 0),
            ">Test".to_string(),
        );
        packet.path.push(CallSign::new("TCPIP*", 0));
        assert!(!filter.should_pass(&packet));

        // Normal packet should pass
        let packet = AprsPacket::new(
            CallSign::new("N0CALL", 0),
            CallSign::new("APRS", 0),
            ">Normal packet".to_string(),
        );
        assert!(filter.should_pass(&packet));
    }

    #[test]
    fn test_invalid_regex() {
        let configs = vec![FilterConfig {
            name: "bad".to_string(),
            action: FilterAction::Drop,
            pattern: "[invalid regex".to_string(),
        }];

        assert!(PacketFilter::new(configs).is_err());
    }
}
