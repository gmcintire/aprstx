use crate::aprs::{AprsPacket, CallSign};
use crate::config::BeaconConfig;
use crate::gps::{GpsPosition, GpsTracker};
use crate::router::{PacketSource, RoutedPacket};
use anyhow::Result;
use chrono::{DateTime, Utc};
use log::{debug, info};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};

pub struct BeaconService {
    config: BeaconConfig,
    gps: Arc<GpsTracker>,
    last_position: Option<GpsPosition>,
    last_beacon_time: DateTime<Utc>,
    stationary_count: u32,
}

impl BeaconService {
    pub fn new(config: BeaconConfig, gps: Arc<GpsTracker>) -> Self {
        BeaconService {
            config,
            gps,
            last_position: None,
            last_beacon_time: Utc::now(),
            stationary_count: 0,
        }
    }

    pub async fn run(mut self, tx: mpsc::Sender<RoutedPacket>) -> Result<()> {
        info!("Starting beacon service");

        let mut check_interval = interval(Duration::from_secs(
            self.config.smart_beacon.check_interval as u64,
        ));

        loop {
            check_interval.tick().await;

            if let Some(current_pos) = self.gps.get_position().await {
                if self.should_beacon(&current_pos).await {
                    self.send_beacon(&current_pos, &tx).await?;
                }
            }
        }
    }

    async fn should_beacon(&mut self, current_pos: &GpsPosition) -> bool {
        let now = Utc::now();
        let time_since_last = now.signed_duration_since(self.last_beacon_time);

        // Always beacon if we haven't sent one in max_interval
        if time_since_last.num_seconds() >= self.config.interval as i64 {
            debug!("Beaconing due to max interval");
            return true;
        }

        // Smart beaconing logic
        if self.config.smart_beacon.enabled {
            match &self.last_position {
                None => {
                    // First position - always beacon regardless of min interval
                    debug!("First position beacon");
                    return true;
                }
                Some(last_pos) => {
                    let distance = calculate_distance(last_pos, current_pos);
                    let speed = current_pos.speed.unwrap_or(0.0);

                    // Check if we're moving
                    if distance < 0.01 {
                        // Less than ~10 meters
                        self.stationary_count += 1;

                        // Beacon less frequently when stationary
                        if self.stationary_count > 3
                            && time_since_last.num_seconds()
                                < self.config.smart_beacon.stationary_interval as i64
                        {
                            return false;
                        }
                    } else {
                        self.stationary_count = 0;

                        // Moving - check turn angle
                        if let (Some(last_course), Some(current_course)) =
                            (last_pos.course, current_pos.course)
                        {
                            let turn_angle = angle_difference(last_course, current_course);

                            // Beacon on significant turns
                            if turn_angle > self.config.smart_beacon.turn_angle as f32
                                && speed > self.config.smart_beacon.turn_speed as f32
                            {
                                debug!("Beaconing due to turn: {} degrees", turn_angle);
                                return true;
                            }
                        }

                        // Speed-based beaconing
                        if speed > self.config.smart_beacon.high_speed as f32 {
                            // High speed - beacon more frequently
                            if time_since_last.num_seconds()
                                >= self.config.smart_beacon.high_speed_interval as i64
                            {
                                debug!("High speed beacon");
                                return true;
                            }
                        } else if speed < self.config.smart_beacon.low_speed as f32 {
                            // Low speed - beacon less frequently
                            if time_since_last.num_seconds()
                                >= self.config.smart_beacon.low_speed_interval as i64
                            {
                                debug!("Low speed beacon");
                                return true;
                            }
                        }
                    }
                }
            }
        }

        // Check minimum interval (after smart beaconing checks)
        if time_since_last.num_seconds() < self.config.smart_beacon.min_interval as i64 {
            return false;
        }

        false
    }

    async fn send_beacon(
        &mut self,
        position: &GpsPosition,
        tx: &mpsc::Sender<RoutedPacket>,
    ) -> Result<()> {
        let packet_info = self.format_position_packet(position);

        let source = CallSign::parse(&self.config.callsign).unwrap_or(CallSign::new("N0CALL", 0));

        let mut packet = AprsPacket::new(source, CallSign::new("APRS", 0), packet_info);

        // Add path if configured
        if !self.config.path.is_empty() {
            packet.path = self
                .config
                .path
                .split(',')
                .filter_map(|p| CallSign::parse(p.trim()))
                .collect();
        }

        info!("Sending position beacon: {}", packet);

        let routed = RoutedPacket {
            packet,
            source: PacketSource::Internal,
        };

        let _ = tx.send(routed).await;

        self.last_position = Some(*position);
        self.last_beacon_time = Utc::now();

        Ok(())
    }

    fn format_position_packet(&self, pos: &GpsPosition) -> String {
        let lat = format_latitude(pos.latitude);
        let lon = format_longitude(pos.longitude);

        let timestamp = if self.config.timestamp {
            format!("@{}", pos.timestamp.format("%d%H%Mz"))
        } else {
            "!".to_string()
        };

        let mut info = format!("{}{}{}{}", timestamp, lat, self.config.symbol_table, lon);
        info.push(self.config.symbol);

        // Add course/speed if available and moving
        if let (Some(course), Some(speed)) = (pos.course, pos.speed) {
            if speed > 1.0 {
                info.push_str(&format!("{:03}/{:03}", course as u16, speed as u16));
            }
        }

        // Add altitude if available
        if let Some(alt) = pos.altitude {
            let alt_ft = (alt * 3.28084) as i32;
            info.push_str(&format!("/A={:06}", alt_ft));
        }

        // Add comment
        if !self.config.comment.is_empty() {
            info.push(' ');
            info.push_str(&self.config.comment);
        }

        info
    }
}

fn format_latitude(lat: f64) -> String {
    let lat_abs = lat.abs();
    let degrees = lat_abs as u8;
    let minutes = (lat_abs - degrees as f64) * 60.0;
    let ns = if lat >= 0.0 { 'N' } else { 'S' };

    format!("{:02}{:05.2}{}", degrees, minutes, ns)
}

fn format_longitude(lon: f64) -> String {
    let lon_abs = lon.abs();
    let degrees = lon_abs as u8;
    let minutes = (lon_abs - degrees as f64) * 60.0;
    let ew = if lon >= 0.0 { 'E' } else { 'W' };

    format!("{:03}{:05.2}{}", degrees, minutes, ew)
}

fn calculate_distance(pos1: &GpsPosition, pos2: &GpsPosition) -> f64 {
    // Haversine formula
    let lat1 = pos1.latitude.to_radians();
    let lat2 = pos2.latitude.to_radians();
    let dlat = (pos2.latitude - pos1.latitude).to_radians();
    let dlon = (pos2.longitude - pos1.longitude).to_radians();

    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();

    6371.0 * c // Earth radius in km
}

fn angle_difference(angle1: f32, angle2: f32) -> f32 {
    let diff = (angle2 - angle1).abs();
    if diff > 180.0 {
        360.0 - diff
    } else {
        diff
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SmartBeaconConfig;
    use crate::gps::{GpsPosition, GpsSource, GpsTracker};

    fn create_test_config() -> BeaconConfig {
        BeaconConfig {
            enabled: true,
            callsign: "N0CALL-9".to_string(),
            interval: 600,
            path: "WIDE1-1,WIDE2-2".to_string(),
            symbol_table: '/',
            symbol: '>',
            comment: "Test beacon".to_string(),
            timestamp: true,
            smart_beacon: SmartBeaconConfig::default(),
        }
    }

    fn create_test_position(
        lat: f64,
        lon: f64,
        speed: Option<f32>,
        course: Option<f32>,
    ) -> GpsPosition {
        GpsPosition {
            latitude: lat,
            longitude: lon,
            altitude: Some(100.0),
            speed,
            course,
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn test_format_latitude() {
        assert_eq!(format_latitude(40.7128), "4042.77N");
        assert_eq!(format_latitude(-33.8688), "3352.13S");
        assert_eq!(format_latitude(0.0), "0000.00N");
    }

    #[test]
    fn test_format_longitude() {
        assert_eq!(format_longitude(-74.0060), "07400.36W");
        assert_eq!(format_longitude(139.6503), "13939.02E");
        assert_eq!(format_longitude(0.0), "00000.00E");
        assert_eq!(format_longitude(180.0), "18000.00E");
        assert_eq!(format_longitude(-180.0), "18000.00W");
    }

    #[test]
    fn test_calculate_distance() {
        let pos1 = create_test_position(40.7128, -74.0060, None, None);
        let pos2 = create_test_position(40.7128, -74.0060, None, None);
        assert!(calculate_distance(&pos1, &pos2) < 0.001);

        let pos3 = create_test_position(40.7589, -73.9851, None, None);
        let distance = calculate_distance(&pos1, &pos3);
        assert!(distance > 5.0 && distance < 6.0); // About 5.2 km
    }

    #[test]
    fn test_angle_difference() {
        assert_eq!(angle_difference(0.0, 45.0), 45.0);
        assert_eq!(angle_difference(45.0, 0.0), 45.0);
        assert_eq!(angle_difference(350.0, 10.0), 20.0);
        assert_eq!(angle_difference(10.0, 350.0), 20.0);
        assert_eq!(angle_difference(0.0, 180.0), 180.0);
        assert_eq!(angle_difference(90.0, 270.0), 180.0);
    }

    #[tokio::test]
    async fn test_should_beacon_first_position() {
        let config = create_test_config();
        let gps = Arc::new(GpsTracker::new(GpsSource::None));
        let mut beacon = BeaconService::new(config, gps);

        let pos = create_test_position(40.7128, -74.0060, Some(0.0), Some(0.0));
        assert!(beacon.should_beacon(&pos).await);
    }

    #[tokio::test]
    async fn test_should_beacon_max_interval() {
        let config = create_test_config();
        let gps = Arc::new(GpsTracker::new(GpsSource::None));
        let mut beacon = BeaconService::new(config, gps);

        let pos = create_test_position(40.7128, -74.0060, Some(0.0), Some(0.0));
        beacon.last_position = Some(pos);
        beacon.last_beacon_time = Utc::now() - chrono::Duration::seconds(700);

        assert!(beacon.should_beacon(&pos).await);
    }

    #[tokio::test]
    async fn test_should_beacon_min_interval() {
        let config = create_test_config();
        let gps = Arc::new(GpsTracker::new(GpsSource::None));
        let mut beacon = BeaconService::new(config, gps);

        let pos = create_test_position(40.7128, -74.0060, Some(0.0), Some(0.0));
        beacon.last_position = Some(pos);
        beacon.last_beacon_time = Utc::now() - chrono::Duration::seconds(10);

        assert!(!beacon.should_beacon(&pos).await);
    }

    #[tokio::test]
    async fn test_should_beacon_turn() {
        let mut config = create_test_config();
        config.smart_beacon.enabled = true;
        config.smart_beacon.turn_angle = 20;
        config.smart_beacon.turn_speed = 5;

        let gps = Arc::new(GpsTracker::new(GpsSource::None));
        let mut beacon = BeaconService::new(config, gps);

        let pos1 = create_test_position(40.7128, -74.0060, Some(10.0), Some(0.0));
        beacon.last_position = Some(pos1);
        beacon.last_beacon_time = Utc::now() - chrono::Duration::seconds(35);

        let pos2 = create_test_position(40.7130, -74.0062, Some(10.0), Some(45.0));
        assert!(beacon.should_beacon(&pos2).await);
    }

    #[tokio::test]
    async fn test_should_beacon_high_speed() {
        let mut config = create_test_config();
        config.smart_beacon.enabled = true;
        config.smart_beacon.high_speed = 60;
        config.smart_beacon.high_speed_interval = 60;

        let gps = Arc::new(GpsTracker::new(GpsSource::None));
        let mut beacon = BeaconService::new(config, gps);

        let pos = create_test_position(40.7128, -74.0060, Some(70.0), Some(0.0));
        beacon.last_position = Some(create_test_position(
            40.7100,
            -74.0050,
            Some(70.0),
            Some(0.0),
        ));
        beacon.last_beacon_time = Utc::now() - chrono::Duration::seconds(65);

        assert!(beacon.should_beacon(&pos).await);
    }

    #[test]
    fn test_format_position_packet() {
        let config = create_test_config();
        let gps = Arc::new(GpsTracker::new(GpsSource::None));
        let beacon = BeaconService::new(config, gps);

        let pos = create_test_position(40.7128, -74.0060, Some(50.0), Some(90.0));
        let packet = beacon.format_position_packet(&pos);

        assert!(packet.starts_with('@'));
        assert!(packet.contains("4042.77N/07400.36W>"));
        assert!(packet.contains("090/050"));
        assert!(packet.contains("/A=000328"));
        assert!(packet.contains("Test beacon"));
    }

    #[test]
    fn test_format_position_packet_stationary() {
        let mut config = create_test_config();
        config.timestamp = false;
        let gps = Arc::new(GpsTracker::new(GpsSource::None));
        let beacon = BeaconService::new(config, gps);

        let pos = create_test_position(40.7128, -74.0060, Some(0.5), None);
        let packet = beacon.format_position_packet(&pos);

        assert!(packet.starts_with('!'));
        assert!(!packet.contains("000/000"));
    }
}
