#!/usr/bin/env bash
# deploy/setup.sh — install Quadlet units and config files on the host.
# Run as root or with sudo. Tested on Fedora 40+ / RHEL 9+ with Podman 5+.
set -euo pipefail

CONFIG_DIR=/etc/sports-log
QUADLET_DIR=/etc/containers/systemd   # system-wide Quadlets (rootful)
# For rootless Podman use: ~/.config/containers/systemd/

echo "==> Creating config directory at $CONFIG_DIR"
install -d -m 750 "$CONFIG_DIR"
install -d -m 750 "$CONFIG_DIR/grafana"

echo "==> Copying env files (edit these before starting services)"
for f in db.env app.env; do
    if [ ! -f "$CONFIG_DIR/$f" ]; then
        install -m 640 "$(dirname "$0")/$f.example" "$CONFIG_DIR/$f"
        echo "    Created $CONFIG_DIR/$f — EDIT BEFORE STARTING"
    else
        echo "    $CONFIG_DIR/$f already exists, skipping"
    fi
done

echo "==> Copying Prometheus config"
install -m 644 "$(dirname "$0")/prometheus/prometheus.yml" "$CONFIG_DIR/prometheus.yml"

echo "==> Copying Grafana provisioning"
cp -r "$(dirname "$0")/grafana/provisioning" "$CONFIG_DIR/grafana/"
cp -r "$(dirname "$0")/grafana/dashboards"   "$CONFIG_DIR/grafana/"

echo "==> Installing Quadlet unit files"
install -d "$QUADLET_DIR"
install -m 644 "$(dirname "$0")"/quadlets/* "$QUADLET_DIR/"

echo "==> Reloading systemd"
systemctl daemon-reload

echo ""
echo "Setup complete. Next steps:"
echo "  1. Edit $CONFIG_DIR/db.env and $CONFIG_DIR/app.env"
echo "  2. Build and tag the app image:"
echo "       podman build -t localhost/sports-log:latest /path/to/sports-log"
echo "  3. Start services in order:"
echo "       systemctl start postgres"
echo "       systemctl start sports-log"
echo "       systemctl start prometheus"
echo "       systemctl start grafana"
echo "  4. Enable on boot:"
echo "       systemctl enable postgres sports-log prometheus grafana"
echo "  5. Access Grafana at http://<host>:3001 (default admin password in grafana_admin_password secret)"
