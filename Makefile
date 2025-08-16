.PHONY: all build test coverage clean fmt lint check install help

# Default target
all: fmt lint test build

# Build the project
build:
	@echo "Building project..."
	@cargo build --release

# Run all tests
test:
	@echo "Running tests..."
	@cargo test --all-features --verbose

# Run tests with coverage report using llvm-cov
coverage:
	@echo "Running tests with coverage..."
	@rustup component add llvm-tools-preview
	@cargo install cargo-llvm-cov
	@cargo llvm-cov --all-features --workspace --html
	@echo "Coverage report generated in target/llvm-cov/html/index.html"

# Run specific test
test-one:
	@cargo test $(TEST) -- --nocapture

# Clean build artifacts
clean:
	@echo "Cleaning..."
	@cargo clean
	@rm -f tarpaulin-report.html lcov.info cobertura.xml

# Format code
fmt:
	@echo "Formatting code..."
	@cargo fmt

# Run linter
lint:
	@echo "Running clippy..."
	@cargo clippy -- -D warnings

# Run all checks (format check, lint, test)
check:
	@echo "Running all checks..."
	@cargo fmt -- --check
	@cargo clippy -- -D warnings
	@cargo test --all-features

# Install the binary
install:
	@echo "Installing aprstx..."
	@cargo install --path .

# Run benchmarks
bench:
	@echo "Running benchmarks..."
	@cargo bench

# Generate documentation
doc:
	@echo "Generating documentation..."
	@cargo doc --all-features --no-deps --open

# Run with example config
run-example:
	@echo "Running with example config..."
	@cargo run -- --config aprstx.conf.example --debug --foreground

# Development mode with auto-reload
dev:
	@echo "Starting in development mode..."
	@cargo watch -x 'run -- --config aprstx.conf.example --debug --foreground'

# Security audit
audit:
	@echo "Running security audit..."
	@cargo audit

# Update dependencies
update:
	@echo "Updating dependencies..."
	@cargo update
	@cargo audit

# Check for outdated dependencies
outdated:
	@cargo outdated

# Create a release build for all targets
release:
	@echo "Building releases..."
	@cargo build --release --target x86_64-unknown-linux-gnu
	@cargo build --release --target x86_64-pc-windows-gnu
	@cargo build --release --target x86_64-apple-darwin

# Help target
help:
	@echo "Available targets:"
	@echo "  all        - Format, lint, test, and build (default)"
	@echo "  build      - Build the project in release mode"
	@echo "  test       - Run all tests"
	@echo "  coverage   - Run tests with coverage report"
	@echo "  test-one   - Run specific test (use TEST=test_name)"
	@echo "  clean      - Clean build artifacts"
	@echo "  fmt        - Format code"
	@echo "  lint       - Run clippy linter"
	@echo "  check      - Run all checks (format, lint, test)"
	@echo "  install    - Install the binary"
	@echo "  bench      - Run benchmarks"
	@echo "  doc        - Generate and open documentation"
	@echo "  run-example - Run with example configuration"
	@echo "  dev        - Development mode with auto-reload (requires cargo-watch)"
	@echo "  audit      - Run security audit"
	@echo "  update     - Update dependencies"
	@echo "  outdated   - Check for outdated dependencies"
	@echo "  release    - Build for all targets"
	@echo "  help       - Show this help message"