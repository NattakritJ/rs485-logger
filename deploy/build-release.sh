#!/usr/bin/env bash
set -euo pipefail

# Default target: Raspberry Pi 4/5 (aarch64, 64-bit OS)
# Override: TARGET=armv7-unknown-linux-gnueabihf ./deploy/build-release.sh
TARGET="${TARGET:-aarch64-unknown-linux-gnu}"

echo "=== rs485-logger release build ==="
echo "Target: $TARGET"
echo ""

# Check cross is installed
if ! command -v cross &>/dev/null; then
    echo "ERROR: 'cross' not found."
    echo "Install: cargo install cross --git https://github.com/cross-rs/cross"
    echo ""
    echo "Alternatively, compile natively on the Raspberry Pi:"
    echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    echo "  cargo build --release"
    exit 1
fi

# Check Docker is running (cross requires it)
if ! docker info &>/dev/null 2>&1; then
    echo "ERROR: Docker is not running. cross requires Docker."
    echo ""
    echo "Alternatively, compile natively on the Raspberry Pi:"
    echo "  cargo build --release"
    exit 1
fi

# Build
cross build --target "$TARGET" --release

BINARY="target/$TARGET/release/rs485-logger"

echo ""
echo "=== Build complete ==="
echo "Binary: $BINARY"
echo "Size:   $(du -sh "$BINARY" | cut -f1)"
echo ""
echo "Deploy steps:"
echo "  1. Copy binary to Pi:      scp $BINARY pi@<PI_IP>:~/rs485-logger"
echo "  2. Copy deploy scripts:    scp deploy/*.sh deploy/*.service deploy/*.rules pi@<PI_IP>:~/deploy/"
echo "  3. Run install script:     ssh pi@<PI_IP> 'sudo ~/deploy/install.sh ~/rs485-logger'"
