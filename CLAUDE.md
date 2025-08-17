# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

aprstx is a high-performance APRS daemon written in Rust that provides I-gate and digipeater functionality. It uses async/await architecture with Tokio for concurrent operations and supports mobile operations with GPS and smart beaconing.

## Build and Development Commands

### Essential Commands

```bash
# Run all tests
cargo test

# Run a single test
cargo test test_name -- --exact

# Run tests from a specific module
cargo test module_name::

# Build release binary
cargo build --release

# Run with example config
cargo run -- -c aprstx.conf.example

# Format, lint, test, and build (recommended before commits)
make all

# Development mode with auto-reload
make dev

# Generate test coverage report
make coverage

# Run linter (MUST pass before committing)
cargo clippy -- -D warnings

# Format code (MUST run before committing)
cargo fmt
```

### IMPORTANT: Pre-commit Checklist

**ALWAYS run these commands before saying any task is complete or committing code:**

```bash
# 1. Format all code
cargo fmt

# 2. Run clippy and ensure no warnings
cargo clippy -- -D warnings

# 3. Run all tests
cargo test

# 4. Build to ensure it compiles
cargo build --release

# 5. Only commit if ALL above commands pass successfully
```

If any of these fail, fix the issues before proceeding. Never commit code that:
- Has formatting issues (cargo fmt modifies files)
- Has clippy warnings
- Fails tests
- Doesn't compile

### Makefile Targets

- `make all` - Complete build pipeline (format → lint → test → build) - **USE THIS BEFORE COMMITS**
- `make test` - Run all tests with verbose output
- `make lint` - Run clippy with warnings as errors
- `make dev` - Watch mode for development (requires cargo-watch)
- `make audit` - Security vulnerability check
- `make doc` - Generate and open documentation

## Architecture Overview

### Core Architecture Pattern

The system follows a hub-and-spoke architecture centered around the **PacketRouter**:

```
Serial Ports → PacketRouter ← APRS-IS Network
               ↓     ↑     ↓
         Digipeater Message Handler
               ↓           ↓
            Beacon    Telemetry
```

### Key Components and Their Interactions

1. **PacketRouter** (`src/router.rs`)
   - Central routing hub that receives `RoutedPacket` structs
   - Routes packets based on source (SerialPort, AprsIs, Internal) 
   - Implements duplicate detection and packet filtering
   - Manages I-gate functionality (RF ↔ APRS-IS bridging)

2. **Serial Communication** (`src/serial/`)
   - `SerialManager` handles multiple serial ports concurrently
   - KISS protocol codec for TNC communication
   - AX.25 frame encoding/decoding for RF packets
   - Pure Rust implementation without system dependencies

3. **Network Module** (`src/network.rs`)
   - Maintains persistent APRS-IS connection with auto-reconnect
   - Implements APRS-IS protocol (login, filters, keep-alive)
   - Bidirectional packet flow with proper filtering

4. **Configuration System** (`src/config.rs`)
   - TOML-based configuration loaded at startup
   - Supports multiple serial ports, APRS-IS settings, filters
   - Hot-reload not supported - requires restart

### Critical Implementation Details

1. **Packet Flow**:
   - All packets are wrapped in `RoutedPacket` with source identification
   - Packets flow through `mpsc` channels between components
   - `broadcast` channels used for fan-out (RF transmission)

2. **Error Handling**:
   - Uses `anyhow::Result` for error propagation
   - Components designed to be resilient - errors logged but don't crash
   - Serial/network failures trigger reconnection attempts

3. **Async Patterns**:
   - All I/O operations are async using Tokio
   - Each major component runs in its own task
   - Graceful shutdown via channel closure propagation

4. **Testing Approach**:
   - Unit tests colocated with modules (`#[cfg(test)]`)
   - Integration tests in `tests/` directory
   - Mock serial ports for testing without hardware

## Common Development Tasks

### Adding a New Feature

1. Identify which component owns the functionality
2. Add configuration fields to `src/config.rs` if needed
3. Implement feature following existing async patterns
4. Add unit tests in the same file
5. Update integration tests if behavior affects packet flow
6. Run `make all` before committing

### Debugging Packet Flow

1. Enable debug logging: `RUST_LOG=debug cargo run`
2. Packet router logs all routing decisions
3. Each component logs packet reception/transmission
4. Use `--dry-run` flag to test without RF transmission

### Working with Serial Ports

- Serial implementation in `src/serial/pure_serial.rs` uses libc/nix
- No external serial dependencies (removed serialport crate)
- KISS protocol implementation handles escaping and framing
- Test with virtual serial ports: `socat -d -d pty,raw,echo=0 pty,raw,echo=0`

### APRS Protocol Notes

- CallSign parsing preserves digipeated markers (*) in the call field
- AX.25 addresses use bit-shifted ASCII with specific encoding rules
- KISS frames: FEND (0xC0) delimiters, command in first byte
- Path handling: WIDEn-N decremented, direct calls marked as used

## Testing Guidelines

### Unit Test Patterns

```rust
#[tokio::test]
async fn test_async_function() {
    // Use tokio::test for async tests
}

#[test]
fn test_sync_function() {
    // Regular test attribute for sync code
}
```

### Integration Test Setup

Tests in `tests/integration_test.rs` use helper functions to:
- Create test configurations
- Spawn isolated daemon instances  
- Simulate packet flow between components
- Verify end-to-end behavior

## Known Issues and Workarounds

1. **Windows Serial Ports**: Use `\\.\COM1` format for port names
2. **GPS Timing**: Allow 5-10 seconds for GPS fix acquisition
3. **APRS-IS Filters**: Server-side filters may take time to activate
4. **Test Flakiness**: Some tests depend on timing - use `cargo test -- --test-threads=1` if needed

## Performance Considerations

- Packet routing is designed for low latency (<1ms typical)
- Viscous delay prevents duplicate transmissions
- Smart beaconing reduces RF congestion
- Memory usage is bounded - old packets are cleaned up periodically