# aprstx - APRS Daemon in Rust

A high-performance APRS (Automatic Packet Reporting System) daemon written in Rust, providing I-gate and digipeater functionality similar to aprx.

## Features

- **I-Gate Functionality**: Bidirectional gateway between RF and APRS-IS
- **Smart Digipeater**: Supports WIDEn-N digipeating with viscous delay
- **Multiple Interfaces**: Supports multiple serial ports with KISS or TNC2 protocols
- **Packet Filtering**: Configurable filters including RFONLY, NOGATE, TCPIP
- **Telemetry**: Automatic telemetry reporting with packet statistics
- **Message Handling**: Processes APRS messages with acknowledgments
- **GPS Support**: Serial NMEA, gpsd, or fixed position
- **Smart Beaconing**: Dynamic position reporting based on speed and direction
- **Mobile Operation**: Full functionality while moving between locations
- **Async Architecture**: Built on Tokio for high performance

## Building

aprstx is written in pure Rust with no system package dependencies required!

### Build

```bash
cargo build --release
```

The binary will be created at `target/release/aprstx`

## Configuration

Copy `aprstx.conf.example` to `/etc/aprstx.conf` and edit for your setup:

```toml
mycall = "N0CALL-10"

[[serial_ports]]
name = "vhf"
device = "/dev/ttyUSB0"
baud_rate = 9600
protocol = "kiss"
tx_enable = true
rx_enable = true

[aprs_is]
server = "rotate.aprs2.net"
port = 14580
callsign = "N0CALL-10"
passcode = "12345"  # Your APRS-IS passcode
filter = "r/40.7/-74.0/50"
tx_enable = true
rx_enable = true

[digipeater]
enabled = true
mycall = "N0CALL-10"
aliases = ["WIDE1-1", "WIDE2-2"]
viscous_delay = 5
max_hops = 3
```

## Running

```bash
# Run with default config
sudo ./target/release/aprstx

# Run with custom config
sudo ./target/release/aprstx --config /path/to/config.toml

# Run in debug mode
sudo ./target/release/aprstx --debug

# Run in foreground
sudo ./target/release/aprstx --foreground
```

## GPS Configuration

aprstx supports multiple GPS sources for mobile operation:

### Serial NMEA GPS
```toml
[gps]
type = "serial"
device = "/dev/ttyUSB1"
baud_rate = 4800
```

### gpsd Connection
```toml
[gps]
type = "gpsd"
host = "localhost"
port = 2947
```

### Fixed Position
```toml
[gps]
type = "fixed"
position = "40.7128,-74.0060,10"  # lat,lon,altitude_meters
```

## Smart Beaconing

The beacon system supports smart beaconing that adjusts transmission rate based on:
- Speed changes
- Direction changes
- Time elapsed

Configure in the `[beacon.smart_beacon]` section to optimize airtime usage while maintaining good position tracking.

## Mobile Operation

aprstx is designed for mobile operation:
- No need to restart when changing locations
- Automatic position updates from GPS
- Smart beaconing reduces unnecessary transmissions
- Full I-gate and digipeater functionality while mobile

## Architecture

- **Packet Router**: Central hub for routing packets between components
- **Serial Module**: Handles KISS and TNC2 serial port protocols
- **Network Module**: Manages APRS-IS connections
- **Digipeater Module**: Implements smart digipeating with viscous delay
- **Filter Module**: Applies configurable packet filters
- **Telemetry Module**: Collects and reports statistics
- **Message Module**: Handles APRS message processing
- **GPS Module**: Handles position tracking from various sources
- **Beacon Module**: Implements smart beaconing algorithm

## License

MIT License - See LICENSE file for details