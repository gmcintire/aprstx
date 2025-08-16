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

## Installation

### APT Repository Installation (Recommended)

For Debian 11 (Bullseye), 12 (Bookworm), or 13 (Trixie):

```bash
# Quick install - adds repository and installs aprstx
curl -fsSL https://gmcintire.github.io/aprstx/install.sh | sudo bash
```

Or manually add the repository:

```bash
# Add repository key
curl -fsSL https://gmcintire.github.io/aprstx/repository-key.asc | sudo apt-key add -

# Add repository to sources
echo "deb https://gmcintire.github.io/aprstx $(lsb_release -cs) main" | sudo tee /etc/apt/sources.list.d/aprstx.list

# Update and install
sudo apt update
sudo apt install aprstx
```

### Development Builds

Automated development builds from the main branch are available:

```bash
# Add dev repository key
curl -fsSL https://gmcintire.github.io/aprstx/dev/dev-repository-key.asc | sudo apt-key add -

# Add dev repository
echo "deb https://gmcintire.github.io/aprstx/dev dev main" | sudo tee /etc/apt/sources.list.d/aprstx-dev.list

# Install development version
sudo apt update
sudo apt install aprstx
```

⚠️ **Warning**: Development builds are automatically generated from each commit to main and may be unstable.

### Direct Package Download

Download .deb packages directly from [GitHub Releases](https://github.com/gmcintire/aprstx/releases):

```bash
# Download and install
wget https://github.com/gmcintire/aprstx/releases/latest/download/aprstx_VERSION_CODENAME_ARCH.deb
sudo dpkg -i aprstx_*.deb
sudo apt-get install -f  # Install dependencies if needed
```

### What the Package Provides

The Debian package will:
- Install the binary to `/usr/bin/aprstx`
- Create a system user `aprstx` with access to serial ports
- Install a systemd service
- Set up proper permissions (no sudo required!)
- Install udev rules for common TNC devices
- Enable automatic updates via apt

### Raspberry Pi Installation

The APT repository includes pre-built binaries for Raspberry Pi:

- **Raspberry Pi 4**: Use either `armhf` (32-bit) or `arm64` (64-bit) packages
- **Raspberry Pi 5**: Use `arm64` (64-bit) packages only

The installation script automatically detects your Pi's architecture:

```bash
# Same installation command works on all Raspberry Pi models
curl -fsSL https://gmcintire.github.io/aprstx/install.sh | sudo bash
```

For optimal performance on Raspberry Pi:
- Use 64-bit Raspberry Pi OS when possible (better performance)
- The `arm64` binaries are optimized for newer Pi models
- Serial port performance is excellent on all Pi models

### Building from Source

aprstx is written in pure Rust with no system package dependencies required!

```bash
cargo build --release
```

The binary will be created at `target/release/aprstx`

## Configuration

For Debian package installations, edit `/etc/aprstx/aprstx.conf`:

```bash
sudo nano /etc/aprstx/aprstx.conf
```

For manual installations, copy `aprstx.conf.example` to `/etc/aprstx.conf` and edit:

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

### Debian Package Installation

```bash
# Start the service
sudo systemctl start aprstx

# Enable at boot
sudo systemctl enable aprstx

# Check status
sudo systemctl status aprstx

# View logs
sudo journalctl -u aprstx -f
```

### Manual Installation

```bash
# Run with default config (requires sudo for serial port access)
sudo ./target/release/aprstx

# Run with custom config
sudo ./target/release/aprstx --config /path/to/config.toml

# Run in debug mode
sudo ./target/release/aprstx --debug

# Run in foreground
sudo ./target/release/aprstx --foreground
```

Note: The Debian package configures the service to run as the `aprstx` user with proper permissions, so sudo is not required when using systemctl.

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

GNU General Public License v3.0 (GPLv3) - See LICENSE.md file for details