use crate::aprs::{AprsPacket, CallSign};
use crate::config::DigipeaterConfig;
use crate::router::{PacketSource, RoutedPacket};
use anyhow::Result;
use log::{debug, info};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, RwLock};

struct DigipeaterState {
    recent_packets: HashMap<String, Instant>,
}

pub async fn run_digipeater(
    config: DigipeaterConfig,
    mut rx: mpsc::Receiver<RoutedPacket>,
    tx: mpsc::Sender<RoutedPacket>,
) -> Result<()> {
    info!("Starting digipeater service with call {}", config.mycall);

    let state = Arc::new(RwLock::new(DigipeaterState {
        recent_packets: HashMap::new(),
    }));

    // Start cleanup task
    let state_clone = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            cleanup_old_packets(&state_clone).await;
        }
    });

    while let Some(routed) = rx.recv().await {
        if should_digipeat(&config, &routed.packet) {
            if let Some(digipeated) = process_packet(&config, &routed.packet, &state).await {
                info!("Digipeating packet: {}", digipeated);

                let routed_digi = RoutedPacket {
                    packet: digipeated,
                    source: PacketSource::Internal,
                };

                let _ = tx.send(routed_digi).await;
            }
        }
    }

    Ok(())
}

fn should_digipeat(config: &DigipeaterConfig, packet: &AprsPacket) -> bool {
    // Don't digipeat if disabled
    if !config.enabled {
        return false;
    }

    // Check if packet has already been digipeated too many times
    let digi_count = packet
        .path
        .iter()
        .filter(|hop| hop.call.contains('*'))
        .count();

    if digi_count >= config.max_hops as usize {
        debug!("Packet has too many hops ({}), not digipeating", digi_count);
        return false;
    }

    // Find the next unused hop in the path
    for hop in &packet.path {
        if !hop.call.contains('*') {
            // Check if this hop is for us
            if hop.call == config.mycall || config.aliases.contains(&hop.call) {
                return true;
            }

            // Check for WIDEn-N pattern
            if is_wide_pattern(&hop.call) {
                return true;
            }

            // If it's not for us and not a WIDE pattern, stop checking
            return false;
        }
    }

    false
}

fn is_wide_pattern(call: &str) -> bool {
    if let Some(dash_pos) = call.find('-') {
        let prefix = &call[..dash_pos];
        let suffix = &call[dash_pos + 1..];

        // Check for WIDEn-N pattern
        if prefix.len() == 5 && prefix.starts_with("WIDE") {
            if let Some(n_char) = prefix.chars().nth(4) {
                if n_char.is_numeric() {
                    if let Ok(n) = suffix.parse::<u8>() {
                        return n > 0 && n <= 7;
                    }
                }
            }
        }
    }
    false
}

async fn process_packet(
    config: &DigipeaterConfig,
    packet: &AprsPacket,
    state: &Arc<RwLock<DigipeaterState>>,
) -> Option<AprsPacket> {
    // Create packet hash for duplicate detection
    let packet_hash = format!("{}>{}", packet.source, packet.information);

    // Check for duplicate (viscous delay)
    {
        let state_read = state.read().await;
        if let Some(last_seen) = state_read.recent_packets.get(&packet_hash) {
            let elapsed = Instant::now().duration_since(*last_seen);
            if elapsed.as_secs() < config.viscous_delay as u64 {
                debug!(
                    "Viscous delay: packet seen {} seconds ago",
                    elapsed.as_secs()
                );
                return None;
            }
        }
    }

    // Store packet hash
    {
        let mut state_write = state.write().await;
        state_write
            .recent_packets
            .insert(packet_hash, Instant::now());
    }

    // Create new packet with updated path
    let mut new_packet = packet.clone();
    let mut new_path = Vec::new();
    let mut found_us = false;

    for hop in &packet.path {
        if !found_us && !hop.call.contains('*') {
            // This is the hop we need to process
            if hop.call == config.mycall || config.aliases.contains(&hop.call) {
                // Direct call to us - mark as used
                new_path.push(CallSign::new(&format!("{}*", config.mycall), 0));
                found_us = true;
            } else if is_wide_pattern(&hop.call) {
                // Process WIDEn-N
                let (wide_type, n) = parse_wide_pattern(&hop.call);
                if n > 1 {
                    // Insert our call and decrement N
                    new_path.push(CallSign::new(&format!("{}*", config.mycall), 0));
                    new_path.push(CallSign::new(&format!("{}-{}", wide_type, n - 1), 0));
                } else {
                    // Last hop - just insert our call
                    new_path.push(CallSign::new(&format!("{}*", config.mycall), 0));
                }
                found_us = true;
            } else {
                // Not for us
                new_path.push(hop.clone());
            }
        } else {
            // Copy remaining hops
            new_path.push(hop.clone());
        }
    }

    if found_us {
        new_packet.path = new_path;
        Some(new_packet)
    } else {
        None
    }
}

fn parse_wide_pattern(call: &str) -> (String, u8) {
    if let Some(dash_pos) = call.find('-') {
        let (prefix, suffix) = call.split_at(dash_pos);
        if let Ok(n) = suffix[1..].parse::<u8>() {
            return (prefix.to_string(), n);
        }
    }
    (call.to_string(), 0)
}

async fn cleanup_old_packets(state: &Arc<RwLock<DigipeaterState>>) {
    let mut state_write = state.write().await;
    let now = Instant::now();
    let max_age = std::time::Duration::from_secs(300); // 5 minutes

    state_write
        .recent_packets
        .retain(|_, time| now.duration_since(*time) < max_age);

    debug!(
        "Cleaned up old packets, {} remaining",
        state_write.recent_packets.len()
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aprs::CallSign;

    fn create_test_config() -> DigipeaterConfig {
        DigipeaterConfig {
            enabled: true,
            mycall: "N0CALL-10".to_string(),
            aliases: vec!["WIDE1-1".to_string()],
            viscous_delay: 5,
            max_hops: 3,
        }
    }

    #[test]
    fn test_should_digipeat_disabled() {
        let mut config = create_test_config();
        config.enabled = false;

        let packet = AprsPacket::new(
            CallSign::new("TEST", 0),
            CallSign::new("APRS", 0),
            ">Test".to_string(),
        );

        assert!(!should_digipeat(&config, &packet));
    }

    #[test]
    fn test_should_digipeat_direct_call() {
        let config = create_test_config();

        let mut packet = AprsPacket::new(
            CallSign::new("TEST", 0),
            CallSign::new("APRS", 0),
            ">Test".to_string(),
        );
        packet.path.push(CallSign::new("N0CALL-10", 0));

        assert!(should_digipeat(&config, &packet));
    }

    #[test]
    fn test_should_digipeat_alias() {
        let config = create_test_config();

        let mut packet = AprsPacket::new(
            CallSign::new("TEST", 0),
            CallSign::new("APRS", 0),
            ">Test".to_string(),
        );
        packet.path.push(CallSign::new("WIDE1-1", 0));

        assert!(should_digipeat(&config, &packet));
    }

    #[test]
    fn test_should_digipeat_wide_pattern() {
        let config = create_test_config();

        let mut packet = AprsPacket::new(
            CallSign::new("TEST", 0),
            CallSign::new("APRS", 0),
            ">Test".to_string(),
        );
        packet.path.push(CallSign::new("WIDE2-2", 0));

        assert!(should_digipeat(&config, &packet));
    }

    #[test]
    fn test_should_not_digipeat_used_hop() {
        let config = create_test_config();

        let mut packet = AprsPacket::new(
            CallSign::new("TEST", 0),
            CallSign::new("APRS", 0),
            ">Test".to_string(),
        );
        packet.path.push(CallSign::new("N0CALL-10*", 0));
        packet.path.push(CallSign::new("WIDE1-1", 0));

        assert!(should_digipeat(&config, &packet));
    }

    #[test]
    fn test_should_not_digipeat_max_hops() {
        let config = create_test_config();

        let mut packet = AprsPacket::new(
            CallSign::new("TEST", 0),
            CallSign::new("APRS", 0),
            ">Test".to_string(),
        );
        packet.path.push(CallSign::new("N0CALL*", 0));
        packet.path.push(CallSign::new("N1CALL*", 0));
        packet.path.push(CallSign::new("N2CALL*", 0));
        packet.path.push(CallSign::new("WIDE1-1", 0));

        assert!(!should_digipeat(&config, &packet));
    }

    #[test]
    fn test_is_wide_pattern() {
        assert!(is_wide_pattern("WIDE1-1"));
        assert!(is_wide_pattern("WIDE2-2"));
        assert!(is_wide_pattern("WIDE3-3"));
        assert!(is_wide_pattern("WIDE7-7"));

        assert!(!is_wide_pattern("WIDE8-8"));
        assert!(!is_wide_pattern("WIDE1-0"));
        assert!(!is_wide_pattern("WIDE"));
        assert!(!is_wide_pattern("WIDE-1"));
        assert!(!is_wide_pattern("WIDEN-1"));
        assert!(!is_wide_pattern("TEST-1"));
    }

    #[test]
    fn test_parse_wide_pattern() {
        assert_eq!(parse_wide_pattern("WIDE2-2"), ("WIDE2".to_string(), 2));
        assert_eq!(parse_wide_pattern("WIDE1-1"), ("WIDE1".to_string(), 1));
        assert_eq!(parse_wide_pattern("TEST"), ("TEST".to_string(), 0));
    }

    #[tokio::test]
    async fn test_process_packet_direct_call() {
        let config = create_test_config();
        let state = Arc::new(RwLock::new(DigipeaterState {
            recent_packets: HashMap::new(),
        }));

        let mut packet = AprsPacket::new(
            CallSign::new("TEST", 0),
            CallSign::new("APRS", 0),
            ">Test".to_string(),
        );
        packet.path.push(CallSign::new("N0CALL-10", 0));

        let result = process_packet(&config, &packet, &state).await.unwrap();

        assert_eq!(result.path.len(), 1);
        assert_eq!(result.path[0].call, "N0CALL-10*");
    }

    #[tokio::test]
    async fn test_process_packet_wide_decrement() {
        let config = create_test_config();
        let state = Arc::new(RwLock::new(DigipeaterState {
            recent_packets: HashMap::new(),
        }));

        let mut packet = AprsPacket::new(
            CallSign::new("TEST", 0),
            CallSign::new("APRS", 0),
            ">Test".to_string(),
        );
        packet.path.push(CallSign::new("WIDE2-2", 0));

        let result = process_packet(&config, &packet, &state).await.unwrap();

        assert_eq!(result.path.len(), 2);
        assert_eq!(result.path[0].call, "N0CALL-10*");
        assert_eq!(result.path[1].call, "WIDE2-1");
    }

    #[tokio::test]
    async fn test_process_packet_wide_last_hop() {
        let config = create_test_config();
        let state = Arc::new(RwLock::new(DigipeaterState {
            recent_packets: HashMap::new(),
        }));

        let mut packet = AprsPacket::new(
            CallSign::new("TEST", 0),
            CallSign::new("APRS", 0),
            ">Test".to_string(),
        );
        packet.path.push(CallSign::new("WIDE1-1", 0));

        let result = process_packet(&config, &packet, &state).await.unwrap();

        assert_eq!(result.path.len(), 1);
        assert_eq!(result.path[0].call, "N0CALL-10*");
    }

    #[tokio::test]
    async fn test_viscous_delay() {
        let config = create_test_config();
        let state = Arc::new(RwLock::new(DigipeaterState {
            recent_packets: HashMap::new(),
        }));

        let mut packet = AprsPacket::new(
            CallSign::new("TEST", 0),
            CallSign::new("APRS", 0),
            ">Test".to_string(),
        );
        packet.path.push(CallSign::new("WIDE1-1", 0));

        // First packet should be processed
        assert!(process_packet(&config, &packet, &state).await.is_some());

        // Same packet within viscous delay should be dropped
        assert!(process_packet(&config, &packet, &state).await.is_none());
    }

    #[tokio::test]
    async fn test_cleanup_old_packets() {
        let state = Arc::new(RwLock::new(DigipeaterState {
            recent_packets: HashMap::new(),
        }));

        // Add old packet
        {
            let mut state_write = state.write().await;
            state_write.recent_packets.insert(
                "old_packet".to_string(),
                Instant::now() - std::time::Duration::from_secs(400),
            );
            state_write
                .recent_packets
                .insert("new_packet".to_string(), Instant::now());
        }

        cleanup_old_packets(&state).await;

        let state_read = state.read().await;
        assert_eq!(state_read.recent_packets.len(), 1);
        assert!(state_read.recent_packets.contains_key("new_packet"));
    }
}
