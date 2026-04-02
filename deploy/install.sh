#!/usr/bin/env bash
set -euo pipefail

BINARY_SRC="${1:-./rs485-logger}"   # first arg or default to cwd binary
CONFIG_DIR=/etc/rs485-logger
LOG_DIR=/var/log/rs485-logger
SERVICE_USER=rs485logger
BINARY_DEST=/usr/local/bin/rs485-logger
SERVICE_FILE=/etc/systemd/system/rs485-logger.service
UDEV_RULE=/etc/udev/rules.d/99-rs485.rules

echo "=== rs485-logger install ==="

# 1. Create service user (idempotent)
if ! id "$SERVICE_USER" &>/dev/null; then
    useradd --system --no-create-home --shell /usr/sbin/nologin "$SERVICE_USER"
    echo "Created system user: $SERVICE_USER"
else
    echo "System user already exists: $SERVICE_USER"
fi

# 2. Add service user to dialout group (for RS485/serial port access)
usermod -aG dialout "$SERVICE_USER"
echo "Added $SERVICE_USER to dialout group"

# 3. Install binary
install -m 755 "$BINARY_SRC" "$BINARY_DEST"
echo "Installed binary: $BINARY_DEST"

# 4. Create config directory (operator must populate config.toml)
mkdir -p "$CONFIG_DIR"
chmod 750 "$CONFIG_DIR"
chown "root:$SERVICE_USER" "$CONFIG_DIR"
echo "Created config directory: $CONFIG_DIR"

# 5. Create log directory (matches ReadWritePaths in systemd service unit)
mkdir -p "$LOG_DIR"
chown "$SERVICE_USER:$SERVICE_USER" "$LOG_DIR"
echo "Created log directory: $LOG_DIR"

# 6. Install systemd service
install -m 644 "$(dirname "$0")/rs485-logger.service" "$SERVICE_FILE"
systemctl daemon-reload
systemctl enable rs485-logger.service
echo "Enabled systemd service"

# 7. Install udev rule
install -m 644 "$(dirname "$0")/99-rs485.rules" "$UDEV_RULE"
udevadm control --reload-rules
udevadm trigger
echo "Installed udev rule — /dev/ttyRS485 will appear on next adapter plug-in"

echo ""
echo "=== Install complete ==="
echo "Next steps:"
echo "  1. Copy your config:  cp config.toml $CONFIG_DIR/config.toml"
echo "  2. Secure it:         chmod 600 $CONFIG_DIR/config.toml && chown $SERVICE_USER:$SERVICE_USER $CONFIG_DIR/config.toml"
echo "  3. Start daemon:      systemctl start rs485-logger"
echo "  4. Check status:      systemctl status rs485-logger"
echo "  5. Watch logs:        journalctl -u rs485-logger -f"
