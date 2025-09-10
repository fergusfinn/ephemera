#!/bin/bash

# Deploy script for Somnial
set -e

# Record deploy start time
DEPLOY_START=$(date +%s)
METRICS_KEY="${METRICS_KEY:-deploy}"

echo "🔨 Building Somnial for Linux x86_64..."
BUILD_START=$(date +%s)
if [[ "$(uname -m)" == "x86_64" && "$(uname -s)" == "Linux" ]]; then
    SQLX_OFFLINE=1 cargo build --release
    BINARY_PATH="target/release/somnial"
else
    SQLX_OFFLINE=1 cross build --release --target x86_64-unknown-linux-gnu
    BINARY_PATH="target/x86_64-unknown-linux-gnu/release/somnial"
fi
BUILD_END=$(date +%s)
BUILD_DURATION=$((BUILD_END - BUILD_START))

echo "📊 Recording binary size..."
BINARY_SIZE=$(stat -f%z "$BINARY_PATH" 2>/dev/null || stat -c%s "$BINARY_PATH" 2>/dev/null || echo "0")
echo "Binary size: $BINARY_SIZE bytes"
echo "Build duration: ${BUILD_DURATION}s"
curl -X POST "https://charts.somnial.co/$METRICS_KEY/binary_size?value=$BINARY_SIZE" -f -s || echo "⚠️ Failed to record binary size metric"
curl -X POST "https://charts.somnial.co/$METRICS_KEY/build_duration?value=$BUILD_DURATION" -f -s || echo "⚠️ Failed to record build duration metric"

echo "📦 Syncing files to server..."
rsync -avz --progress "$BINARY_PATH" somnial@ubuntu-4gb-nbg1-2:~/
rsync -avz --progress Caddyfile somnial@ubuntu-4gb-nbg1-2:~/

echo "🔧 Installing Caddy and setting up systemd services..."
ssh somnial@ubuntu-4gb-nbg1-2 'bash -s' << 'EOF'
# Install Caddy if not present
if ! command -v caddy &> /dev/null; then
    echo "📦 Installing Caddy..."
    sudo apt update
    sudo apt install -y debian-keyring debian-archive-keyring apt-transport-https curl
    curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' | sudo gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
    curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' | sudo tee /etc/apt/sources.list.d/caddy-stable.list
    sudo apt update
    sudo apt install -y caddy
    echo "✅ Caddy installed"
else
    echo "ℹ️ Caddy already installed"
fi
# Create somnial systemd service
if [ ! -f /etc/systemd/system/somnial.service ]; then
    sudo tee /etc/systemd/system/somnial.service > /dev/null << EOL
[Unit]
Description=Somnial Metrics Service
After=network.target

[Service]
Type=simple
User=somnial
WorkingDirectory=/home/somnial
ExecStart=/home/somnial/somnial
Restart=on-failure
RestartSec=5
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
EOL
    sudo systemctl daemon-reload
    sudo systemctl enable somnial
    echo "✅ Created somnial.service"
else
    echo "ℹ️ somnial.service already exists"
fi

# Create caddy systemd service
if [ ! -f /etc/systemd/system/caddy.service ]; then
    sudo tee /etc/systemd/system/caddy.service > /dev/null << EOL
[Unit]
Description=Caddy
Documentation=https://caddyserver.com/docs/
After=network.target network-online.target
Requires=network-online.target

[Service]
Type=notify
User=caddy
Group=caddy
ExecStart=/usr/bin/caddy run --environ --config /etc/caddy/Caddyfile
ExecReload=/usr/bin/caddy reload --config /etc/caddy/Caddyfile --force
TimeoutStopSec=5s
LimitNOFILE=1048576
LimitNPROC=1048576
PrivateTmp=true
ProtectSystem=full
AmbientCapabilities=CAP_NET_BIND_SERVICE

[Install]
WantedBy=multi-user.target
EOL
    
    # Create caddy user if it doesn't exist
    if ! id "caddy" &>/dev/null; then
        sudo useradd --system --shell /bin/false --home /var/lib/caddy caddy
    fi
    
    sudo systemctl daemon-reload
    sudo systemctl enable caddy
    echo "✅ Created caddy.service"
else
    echo "ℹ️ caddy.service already exists"
fi

# Setup Caddy configuration and permissions
sudo mkdir -p /etc/caddy /var/lib/caddy
sudo mv /home/somnial/Caddyfile /etc/caddy/
sudo chown -R caddy:caddy /var/lib/caddy /etc/caddy
EOF

echo "🚀 Restarting services..."
ssh somnial@ubuntu-4gb-nbg1-2 "sudo systemctl restart somnial"
ssh somnial@ubuntu-4gb-nbg1-2 "sudo systemctl restart caddy"

DEPLOY_END=$(date +%s)
DEPLOY_DURATION=$((DEPLOY_END - DEPLOY_START))

echo "📊 Recording deploy duration..."
echo "Deploy duration: ${DEPLOY_DURATION}s"
curl -X POST "https://charts.somnial.co/$METRICS_KEY/deploy_duration?value=$DEPLOY_DURATION" -f -s || echo "⚠️ Failed to record deploy duration metric"

echo "✅ Deploy complete!"
echo "Check status with: ssh somnial@ubuntu-4gb-nbg1-2 'sudo systemctl status somnial'"

