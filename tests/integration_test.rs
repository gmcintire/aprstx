use aprstx::aprs::{parse_packet, AprsPacket, CallSign};
use aprstx::config::{FilterAction, FilterConfig};
use aprstx::filter::PacketFilter;
use std::sync::Arc;
use tokio::sync::mpsc;

#[tokio::test]
async fn test_packet_flow() {
    // Test that a packet flows through the system correctly
    let packet_str = "N0CALL>APRS,WIDE1-1:>Test packet";
    let packet = parse_packet(packet_str).unwrap();

    assert_eq!(packet.source.call, "N0CALL");
    assert_eq!(packet.destination.call, "APRS");
    assert_eq!(packet.path.len(), 1);
    assert_eq!(packet.path[0].call, "WIDE1");
    assert_eq!(packet.path[0].ssid.0, 1);
    assert_eq!(packet.information, ">Test packet");
}

#[tokio::test]
async fn test_filter_integration() {
    let configs = vec![
        FilterConfig {
            name: "rfonly".to_string(),
            action: FilterAction::Drop,
            pattern: "RFONLY".to_string(),
        },
        FilterConfig {
            name: "emergency".to_string(),
            action: FilterAction::Pass,
            pattern: "EMERGENCY".to_string(),
        },
    ];

    let filter = PacketFilter::new(configs).unwrap();

    // Normal packet should pass
    let packet = AprsPacket::new(
        CallSign::new("N0CALL", 0),
        CallSign::new("APRS", 0),
        ">Normal packet".to_string(),
    );
    assert!(filter.should_pass(&packet));

    // RFONLY packet should be dropped
    let packet = AprsPacket::new(
        CallSign::new("N0CALL", 0),
        CallSign::new("APRS", 0),
        ">Test RFONLY".to_string(),
    );
    assert!(!filter.should_pass(&packet));

    // EMERGENCY packet should pass even with other filters
    let packet = AprsPacket::new(
        CallSign::new("N0CALL", 0),
        CallSign::new("APRS", 0),
        ">EMERGENCY situation".to_string(),
    );
    assert!(filter.should_pass(&packet));
}

#[tokio::test]
async fn test_channel_communication() {
    let (tx, mut rx) = mpsc::channel(10);

    // Send a test packet
    let packet = AprsPacket::new(
        CallSign::new("TEST", 0),
        CallSign::new("APRS", 0),
        ">Test".to_string(),
    );

    tx.send(packet.clone()).await.unwrap();

    // Receive and verify
    let received = rx.recv().await.unwrap();
    assert_eq!(received.source.call, "TEST");
    assert_eq!(received.information, ">Test");
}

#[test]
fn test_config_loading() {
    // This would test loading a config from a string
    let _config_str = r#"
        mycall = "N0CALL-10"
        
        [[serial_ports]]
        name = "test"
        device = "/dev/ttyUSB0"
        baud_rate = 9600
        protocol = "kiss"
        tx_enable = true
        rx_enable = true
        
        [digipeater]
        enabled = true
        mycall = "N0CALL-10"
        aliases = ["WIDE1-1"]
        viscous_delay = 5
        max_hops = 3
        
        [telemetry]
        enabled = false
        interval = 1200
        comment = "Test"
        
        [[filters]]
        name = "test"
        action = "drop"
        pattern = "TEST"
    "#;

    // Note: This would need Config to have a from_str method
    // let config: Config = toml::from_str(config_str).unwrap();
    // assert_eq!(config.mycall, "N0CALL-10");
}

#[tokio::test]
async fn test_message_ack_flow() {
    use aprstx::aprs::packet::DataType;

    // Create a message packet
    let msg_packet = AprsPacket::new(
        CallSign::new("N1CALL", 0),
        CallSign::new("APRS", 0),
        ":N0CALL   :Test message{123".to_string(),
    );

    assert_eq!(msg_packet.data_type, DataType::Message);

    // Verify message format
    let info = &msg_packet.information;
    assert!(info.starts_with(':'));
    assert_eq!(&info[1..10], "N0CALL   ");
    assert!(info.contains("Test message{123"));
}

#[test]
fn test_position_packet_creation() {
    use aprstx::gps::GpsPosition;
    use chrono::Utc;

    let pos = GpsPosition {
        latitude: 40.7128,
        longitude: -74.0060,
        altitude: Some(100.0),
        speed: Some(50.0),
        course: Some(90.0),
        timestamp: Utc::now(),
    };

    // Test position formatting
    assert_eq!(pos.latitude, 40.7128);
    assert_eq!(pos.longitude, -74.0060);
    assert_eq!(pos.altitude, Some(100.0));
}

#[tokio::test]
async fn test_smart_beacon_logic() {
    use aprstx::beacon::BeaconService;
    use aprstx::config::{BeaconConfig, SmartBeaconConfig};
    use aprstx::gps::{GpsPosition, GpsSource, GpsTracker};
    use chrono::Utc;

    let config = BeaconConfig {
        enabled: true,
        callsign: "N0CALL-9".to_string(),
        interval: 600,
        path: "WIDE1-1".to_string(),
        symbol_table: '/',
        symbol: '>',
        comment: "Test".to_string(),
        timestamp: true,
        smart_beacon: SmartBeaconConfig {
            enabled: true,
            check_interval: 5,
            min_interval: 30,
            stationary_interval: 600,
            low_speed: 5,
            low_speed_interval: 300,
            high_speed: 60,
            high_speed_interval: 60,
            turn_angle: 20,
            turn_speed: 5,
        },
    };

    let pos = GpsPosition {
        latitude: 40.7128,
        longitude: -74.0060,
        altitude: Some(100.0),
        speed: Some(0.0),
        course: Some(0.0),
        timestamp: Utc::now(),
    };

    let gps = Arc::new(GpsTracker::new(GpsSource::Fixed(pos)));
    let _beacon = BeaconService::new(config, gps);

    // Test would continue with beacon logic
}

// More comprehensive integration tests would go here...
