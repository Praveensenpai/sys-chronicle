#!/usr/bin/env bash
set -e

REPO="Praveensenpai/sys-chronicle"
BINARY="sys-chronicle"
INSTALL_DIR="$HOME/.local/bin"

echo "[+] Installing $BINARY..."

mkdir -p "$INSTALL_DIR"

if [ -f "Cargo.toml" ]; then
    echo "[+] Local repository detected. Building release binary with Cargo..."
    cargo build --release
    cp target/release/"$BINARY" "$INSTALL_DIR/$BINARY"
else
    echo "[+] Downloading latest release binary from GitHub..."
    LATEST_TAG=$(curl -4 -fL -sS -H "Cache-Control: no-cache" "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')
    if [ -z "$LATEST_TAG" ]; then
        echo "[-] Error: Could not resolve latest release tag."
        exit 1
    fi
    echo "[+] Latest release tag: $LATEST_TAG"
    DOWNLOAD_URL="https://github.com/$REPO/releases/download/$LATEST_TAG/$BINARY-linux-x86_64.tar.gz"
    
    TMP_DIR=$(mktemp -d)
    curl -4 -fL --connect-timeout 10 --retry 3 -sS "$DOWNLOAD_URL" -o "$TMP_DIR/$BINARY.tar.gz"
    tar -xzf "$TMP_DIR/$BINARY.tar.gz" -C "$TMP_DIR"
    mv "$TMP_DIR/$BINARY" "$INSTALL_DIR/$BINARY"
    rm -rf "$TMP_DIR"
fi

chmod +x "$INSTALL_DIR/$BINARY"
echo "✔ Installed $BINARY to $INSTALL_DIR/$BINARY"

"$INSTALL_DIR/$BINARY" install-service
echo "✔ systemd user service configured. Run 'systemctl --user enable --now sys-chronicle.service' to start automatically."
