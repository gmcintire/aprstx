use crate::serial::pure_serial::SerialPort;
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use log::{debug, error, info, warn};
use nmea::Nmea;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader as AsyncBufReader};
use tokio::net::TcpStream;
use tokio::sync::RwLock;

#[derive(Debug, Clone, PartialEq)]
pub enum GpsSource {
    None,
    Fixed(GpsPosition),
    SerialNmea(String, u32), // device, baud
    Gpsd(String, u16),       // host, port
}

#[derive(Debug, Clone, Copy)]
pub struct GpsPosition {
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: Option<f32>,
    pub speed: Option<f32>,  // knots
    pub course: Option<f32>, // degrees
    pub timestamp: DateTime<Utc>,
}

impl PartialEq for GpsPosition {
    fn eq(&self, other: &Self) -> bool {
        (self.latitude - other.latitude).abs() < 0.000001
            && (self.longitude - other.longitude).abs() < 0.000001
    }
}

pub struct GpsTracker {
    source: GpsSource,
    position: Arc<RwLock<Option<GpsPosition>>>,
    nmea_parser: Arc<RwLock<Nmea>>,
}

impl GpsTracker {
    pub fn new(source: GpsSource) -> Self {
        GpsTracker {
            source,
            position: Arc::new(RwLock::new(None)),
            nmea_parser: Arc::new(RwLock::new(Nmea::default())),
        }
    }

    pub async fn get_position(&self) -> Option<GpsPosition> {
        match &self.source {
            GpsSource::Fixed(pos) => Some(*pos),
            _ => *self.position.read().await,
        }
    }

    pub async fn run(&self) -> Result<()> {
        match &self.source {
            GpsSource::None => {
                info!("GPS disabled");
                Ok(())
            }
            GpsSource::Fixed(pos) => {
                info!(
                    "Using fixed position: {:.6}, {:.6}",
                    pos.latitude, pos.longitude
                );
                Ok(())
            }
            GpsSource::SerialNmea(device, baud) => self.run_serial_nmea(device, *baud).await,
            GpsSource::Gpsd(host, port) => self.run_gpsd(host, *port).await,
        }
    }

    async fn run_serial_nmea(&self, device: &str, baud: u32) -> Result<()> {
        info!("Starting GPS NMEA receiver on {} at {} baud", device, baud);

        loop {
            match self.connect_serial_nmea(device, baud).await {
                Ok(_) => {
                    warn!("GPS serial connection closed, reconnecting in 5s...");
                }
                Err(e) => {
                    error!("GPS serial error: {}, reconnecting in 5s...", e);
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }
    }

    async fn connect_serial_nmea(&self, device: &str, baud: u32) -> Result<()> {
        let port = SerialPort::open(device, baud).await?;
        let mut reader = AsyncBufReader::new(port);
        let mut line = String::new();

        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break,
                Ok(_) => {
                    let trimmed = line.trim();
                    if trimmed.starts_with('$') {
                        self.process_nmea_sentence(trimmed).await;
                    }
                }
                Err(e) => {
                    error!("Error reading GPS serial: {}", e);
                    break;
                }
            }
        }

        Ok(())
    }

    async fn run_gpsd(&self, host: &str, port: u16) -> Result<()> {
        info!("Starting gpsd client connecting to {}:{}", host, port);

        loop {
            match self.connect_gpsd(host, port).await {
                Ok(_) => {
                    warn!("gpsd connection closed, reconnecting in 5s...");
                }
                Err(e) => {
                    error!("gpsd connection error: {}, reconnecting in 5s...", e);
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }
    }

    async fn connect_gpsd(&self, host: &str, port: u16) -> Result<()> {
        let stream = TcpStream::connect(format!("{}:{}", host, port)).await?;
        let mut reader = AsyncBufReader::new(stream);
        let mut line = String::new();

        // Send watch command to start receiving data
        reader
            .get_mut()
            .write_all(b"?WATCH={\"enable\":true,\"json\":true}\r\n")
            .await?;

        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break,
                Ok(_) => {
                    self.process_gpsd_json(&line).await;
                }
                Err(e) => {
                    error!("Error reading from gpsd: {}", e);
                    break;
                }
            }
        }

        Ok(())
    }

    async fn process_nmea_sentence(&self, sentence: &str) {
        let mut parser = self.nmea_parser.write().await;

        if let Err(e) = parser.parse(sentence) {
            debug!("Failed to parse NMEA sentence: {}", e);
            return;
        }

        // Check if we have a fix and extract position
        if let Some(lat) = parser.latitude {
            if let Some(lon) = parser.longitude {
                let pos = GpsPosition {
                    latitude: lat,
                    longitude: lon,
                    altitude: parser.altitude,
                    speed: parser.speed_over_ground,
                    course: parser.true_course,
                    timestamp: Utc::now(),
                };

                self.update_position(pos).await;
            }
        }
    }

    async fn process_gpsd_json(&self, json_str: &str) {
        match serde_json::from_str::<serde_json::Value>(json_str) {
            Ok(json) => {
                if json["class"] == "TPV" {
                    if let (Some(lat), Some(lon)) = (json["lat"].as_f64(), json["lon"].as_f64()) {
                        let pos = GpsPosition {
                            latitude: lat,
                            longitude: lon,
                            altitude: json["alt"].as_f64().map(|a| a as f32),
                            speed: json["speed"].as_f64().map(|s| (s * 1.94384) as f32), // m/s to knots
                            course: json["track"].as_f64().map(|c| c as f32),
                            timestamp: Utc::now(),
                        };

                        self.update_position(pos).await;
                    }
                }
            }
            Err(e) => {
                debug!("Failed to parse gpsd JSON: {}", e);
            }
        }
    }

    async fn update_position(&self, new_pos: GpsPosition) {
        let mut position = self.position.write().await;

        let should_log = match &*position {
            None => true,
            Some(old_pos) => {
                (new_pos.latitude - old_pos.latitude).abs() > 0.0001
                    || (new_pos.longitude - old_pos.longitude).abs() > 0.0001
            }
        };

        if should_log {
            info!(
                "GPS position: {:.6}, {:.6} alt={:?}m speed={:?}kts course={:?}°",
                new_pos.latitude,
                new_pos.longitude,
                new_pos.altitude,
                new_pos.speed,
                new_pos.course
            );
        }

        *position = Some(new_pos);
    }
}

pub fn parse_fixed_position(pos_str: &str) -> Result<GpsPosition> {
    let parts: Vec<&str> = pos_str.split(',').collect();
    if parts.len() < 2 {
        return Err(anyhow!(
            "Invalid position format. Use: latitude,longitude[,altitude]"
        ));
    }

    let latitude: f64 = parts[0]
        .trim()
        .parse()
        .map_err(|_| anyhow!("Invalid latitude"))?;
    let longitude: f64 = parts[1]
        .trim()
        .parse()
        .map_err(|_| anyhow!("Invalid longitude"))?;

    let altitude = if parts.len() > 2 {
        Some(
            parts[2]
                .trim()
                .parse()
                .map_err(|_| anyhow!("Invalid altitude"))?,
        )
    } else {
        None
    };

    if !(-90.0..=90.0).contains(&latitude) {
        return Err(anyhow!("Latitude must be between -90 and 90"));
    }

    if !(-180.0..=180.0).contains(&longitude) {
        return Err(anyhow!("Longitude must be between -180 and 180"));
    }

    Ok(GpsPosition {
        latitude,
        longitude,
        altitude,
        speed: None,
        course: None,
        timestamp: Utc::now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_fixed_position() {
        // Valid position with altitude
        let pos = parse_fixed_position("40.7128,-74.0060,100").unwrap();
        assert_eq!(pos.latitude, 40.7128);
        assert_eq!(pos.longitude, -74.0060);
        assert_eq!(pos.altitude, Some(100.0));

        // Valid position without altitude
        let pos = parse_fixed_position("51.5074,-0.1278").unwrap();
        assert_eq!(pos.latitude, 51.5074);
        assert_eq!(pos.longitude, -0.1278);
        assert_eq!(pos.altitude, None);

        // With spaces
        let pos = parse_fixed_position(" 35.6762 , 139.6503 , 50 ").unwrap();
        assert_eq!(pos.latitude, 35.6762);
        assert_eq!(pos.longitude, 139.6503);
        assert_eq!(pos.altitude, Some(50.0));
    }

    #[test]
    fn test_parse_fixed_position_errors() {
        // Missing longitude
        assert!(parse_fixed_position("40.7128").is_err());

        // Empty string
        assert!(parse_fixed_position("").is_err());

        // Invalid latitude
        assert!(parse_fixed_position("not_a_number,-74.0060").is_err());

        // Invalid longitude
        assert!(parse_fixed_position("40.7128,not_a_number").is_err());

        // Invalid altitude
        assert!(parse_fixed_position("40.7128,-74.0060,not_a_number").is_err());

        // Latitude out of range
        assert!(parse_fixed_position("91.0,-74.0060").is_err());
        assert!(parse_fixed_position("-91.0,-74.0060").is_err());

        // Longitude out of range
        assert!(parse_fixed_position("40.7128,181.0").is_err());
        assert!(parse_fixed_position("40.7128,-181.0").is_err());
    }

    #[test]
    fn test_gps_position_equality() {
        let pos1 = GpsPosition {
            latitude: 40.7128,
            longitude: -74.0060,
            altitude: Some(100.0),
            speed: Some(10.0),
            course: Some(180.0),
            timestamp: Utc::now(),
        };

        let pos2 = GpsPosition {
            latitude: 40.7128,
            longitude: -74.0060,
            altitude: Some(200.0),
            speed: Some(20.0),
            course: Some(90.0),
            timestamp: Utc::now(),
        };

        let pos3 = GpsPosition {
            latitude: 40.7129,
            longitude: -74.0060,
            altitude: Some(100.0),
            speed: Some(10.0),
            course: Some(180.0),
            timestamp: Utc::now(),
        };

        assert_eq!(pos1, pos2); // Same lat/lon
        assert_ne!(pos1, pos3); // Different lat
    }

    #[tokio::test]
    async fn test_gps_tracker_fixed() {
        let pos = GpsPosition {
            latitude: 40.7128,
            longitude: -74.0060,
            altitude: Some(100.0),
            speed: None,
            course: None,
            timestamp: Utc::now(),
        };

        let tracker = GpsTracker::new(GpsSource::Fixed(pos));
        let retrieved = tracker.get_position().await.unwrap();

        assert_eq!(retrieved.latitude, 40.7128);
        assert_eq!(retrieved.longitude, -74.0060);
        assert_eq!(retrieved.altitude, Some(100.0));
    }

    #[tokio::test]
    async fn test_gps_tracker_none() {
        let tracker = GpsTracker::new(GpsSource::None);
        assert!(tracker.get_position().await.is_none());
    }

    #[test]
    fn test_nmea_processing() {
        let _tracker = GpsTracker::new(GpsSource::None);

        // Test GGA sentence
        let _gga = "$GPGGA,123519,4807.038,N,01131.000,E,1,08,0.9,545.4,M,46.9,M,,*47";
        // Note: This would need the NMEA parser to be properly initialized
        // and the function to be made testable
    }

    #[test]
    fn test_gpsd_json_processing() {
        let _tracker = GpsTracker::new(GpsSource::None);

        // Test TPV JSON
        let _json = r#"{
            "class": "TPV",
            "device": "/dev/ttyUSB0",
            "mode": 3,
            "time": "2024-01-01T12:00:00Z",
            "lat": 40.7128,
            "lon": -74.0060,
            "alt": 100.0,
            "speed": 5.14444,
            "track": 180.0
        }"#;

        // This would need the process_gpsd_json to be made testable
    }
}
