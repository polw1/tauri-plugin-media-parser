#!/bin/bash
set -e

echo "Setting up Tauri HLS Media Player dependencies for Linux..."

# Detect distro
if [ -f /etc/debian_version ]; then
    echo "Detected: Debian/Ubuntu"
    sudo apt-get update
    sudo apt-get install -y \
        libwebkit2gtk-4.1-dev \
        libappindicator3-dev \
        librsvg2-dev \
        patchelf \
        gstreamer1.0-libav \
        gstreamer1.0-plugins-good \
        fonts-noto-cjk \
        xvfb

elif [ -f /etc/fedora-release ] || [ -f /etc/redhat-release ]; then
    echo "Detected: Fedora/RHEL"
    sudo dnf install -y \
        webkit2gtk4.1-devel \
        libappindicator-gtk3-devel \
        librsvg2-devel \
        patchelf \
        gstreamer1-libav \
        gstreamer1-plugins-good \
        google-noto-sans-cjk-fonts

elif [ -f /etc/arch-release ]; then
    echo "Detected: Arch Linux"
    sudo pacman -S --needed \
        webkit2gtk-4.1 \
        libappindicator-gtk3 \
        librsvg \
        patchelf \
        gst-libav \
        gst-plugins-good \
        noto-fonts-cjk

else
    echo "Unknown distro. Please install manually:"
    echo "  - webkit2gtk 4.1+"
    echo "  - gstreamer libav + good plugins"
    echo "  - Noto CJK fonts for Japanese/Chinese/Korean subtitles"
    echo "  - librsvg, libappindicator, patchelf"
    exit 1
fi

echo ""
echo "✅ Linux dependencies installed!"
echo "Run: npm install && npm run tauri dev"
