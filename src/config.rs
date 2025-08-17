use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub mycall: String,
    pub serial_ports: Vec<SerialPortConfig>,
    pub aprs_is: Option<AprsIsConfig>,
    pub digipeater: DigipeaterConfig,
    pub telemetry: TelemetryConfig,
    pub filters: Vec<FilterConfig>,
    pub gps: Option<GpsConfig>,
    pub beacon: Option<BeaconConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SerialPortConfig {
    pub name: String,
    pub device: String,
    pub baud_rate: u32,
    pub protocol: SerialProtocol,
    pub tx_enable: bool,
    pub rx_enable: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SerialProtocol {
    Kiss,
    Tnc2,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AprsIsConfig {
    pub server: String,
    pub port: u16,
    pub callsign: String,
    pub passcode: String,
    pub filter: Option<String>,
    pub tx_enable: bool,
    pub rx_enable: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DigipeaterConfig {
    pub enabled: bool,
    pub mycall: String,
    pub aliases: Vec<String>,
    pub viscous_delay: u32,
    pub max_hops: u8,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TelemetryConfig {
    pub enabled: bool,
    pub interval: u32,
    pub comment: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FilterConfig {
    pub name: String,
    pub action: FilterAction,
    pub pattern: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FilterAction {
    Drop,
    Pass,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GpsConfig {
    #[serde(rename = "type")]
    pub gps_type: String, // "none", "serial", "gpsd", "fixed"
    pub device: Option<String>,
    pub baud_rate: Option<u32>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub position: Option<String>, // for fixed position: "lat,lon[,alt]"
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BeaconConfig {
    pub enabled: bool,
    pub callsign: String,
    pub interval: u32, // seconds
    pub path: String,
    pub symbol_table: char,
    pub symbol: char,
    pub comment: String,
    pub timestamp: bool,
    pub smart_beacon: SmartBeaconConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SmartBeaconConfig {
    pub enabled: bool,
    pub check_interval: u32,      // How often to check position (seconds)
    pub min_interval: u32,        // Minimum time between beacons
    pub stationary_interval: u32, // Interval when not moving
    pub low_speed: u32,           // Speed threshold (knots)
    pub low_speed_interval: u32,  // Interval at low speed
    pub high_speed: u32,          // High speed threshold
    pub high_speed_interval: u32, // Interval at high speed
    pub turn_angle: u32,          // Degrees to trigger beacon
    pub turn_speed: u32,          // Minimum speed for turn detection
}

impl Default for SmartBeaconConfig {
    fn default() -> Self {
        SmartBeaconConfig {
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
        }
    }
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let contents = std::fs::read_to_string(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!(
                    "Configuration file not found: {}\n\
                         Hint: Copy aprstx.conf.example to {} and edit it with your settings.\n\
                         Or use --config to specify a different path.",
                    path.display(),
                    path.display()
                )
            } else {
                anyhow::anyhow!("Failed to read config file {}: {}", path.display(), e)
            }
        })?;
        let config: Config = toml::from_str(&contents).map_err(|e| {
            anyhow::anyhow!(
                "Failed to parse configuration file {}: {}\n\
                     Hint: Check the TOML syntax. Common issues:\n\
                     - Missing quotes around strings\n\
                     - Incorrect array syntax (use [[section]] for arrays)\n\
                     - Invalid data types for fields",
                path.display(),
                e
            )
        })?;
        Ok(config)
    }
}
