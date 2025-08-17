#!/bin/bash
set -e

echo "APRS Daemon (aprstx) Quick Installer"
echo "===================================="
echo
echo "NOTE: The APT repository is not yet available."
echo "This script will help you build and install from source."
echo

# Check if running as root
if [ "$EUID" -eq 0 ]; then 
    echo "Please run without sudo for building"
    exit 1
fi

# Check for Rust
if ! command -v cargo &> /dev/null; then
    echo "Rust/Cargo not found. Installing..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
fi

# Build the project
echo "Building aprstx..."
cargo build --release

# Install
echo
echo "Installing aprstx (requires sudo)..."
sudo install -Dm755 target/release/aprstx /usr/local/bin/aprstx

# Create config directory
sudo mkdir -p /etc/aprstx

# Copy example config if it doesn't exist
if [ ! -f /etc/aprstx/aprstx.conf ]; then
    sudo cp aprstx.conf.example /etc/aprstx/aprstx.conf
    echo "Example configuration copied to /etc/aprstx/aprstx.conf"
fi

# Create systemd service
if [ -f debian/aprstx.service ]; then
    sudo cp debian/aprstx.service /etc/systemd/system/
    sudo systemctl daemon-reload
    echo "Systemd service installed"
fi

echo
echo "Installation complete!"
echo
echo "Next steps:"
echo "1. Edit configuration: sudo nano /etc/aprstx/aprstx.conf"
echo "2. Start the service: sudo systemctl start aprstx"
echo "3. Enable at boot: sudo systemctl enable aprstx"
echo "4. Check status: sudo systemctl status aprstx"
echo
echo "For the APT repository (when available), visit:"
echo "https://gmcintire.github.io/aprstx/"