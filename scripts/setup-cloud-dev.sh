#!/bin/bash
# setup-cloud-dev.sh
# Sets up system dependencies for cloud docker environments (Claude web, Codex web, etc.)
# Assumes: rustup already installed, base Linux image with apt

set -euo pipefail

echo "=== Setting up Nori CLI cloud development environment ==="

# Determine sudo usage
if [ "$(whoami)" = root ]; then SUDO=; else SUDO=sudo; fi

# System packages via apt
echo "Installing system packages..."
$SUDO apt-get update
$SUDO apt-get install -y \
    clang \
    pkg-config \
    libssl-dev \
    build-essential \
    wget

# Install mold from GitHub releases (Debian package is too outdated)
MOLD_VERSION="2.35.1"
echo "Installing mold ${MOLD_VERSION}..."
wget -O- --timeout=10 --waitretry=3 --retry-connrefused --progress=dot:mega \
    "https://github.com/rui314/mold/releases/download/v${MOLD_VERSION}/mold-${MOLD_VERSION}-$(uname -m)-linux.tar.gz" \
    | $SUDO tar -C /usr/local --strip-components=1 --no-overwrite-dir -xzf -

# Make mold the default linker if not already
if [ "$(realpath /usr/bin/ld)" != /usr/local/bin/mold ]; then
    $SUDO ln -sf /usr/local/bin/mold "$(realpath /usr/bin/ld)" || true
fi

# Rust toolchain setup (assumes rustup is available)
echo "Configuring Rust toolchain..."
rustup default 1.90.0
rustup component add clippy rustfmt rust-src
rustup toolchain install nightly
rustup component add rustfmt --toolchain nightly

# Cargo tools via binstall (faster) or cargo install
echo "Installing cargo tools..."
if ! command -v cargo-binstall &> /dev/null; then
    echo "Installing cargo-binstall..."
    curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash
fi

cargo binstall -y just
cargo binstall -y cargo-nextest
cargo binstall -y cargo-insta
cargo binstall -y sccache

# Configure sccache
echo "Configuring sccache..."
export RUSTC_WRAPPER=sccache
echo 'export RUSTC_WRAPPER=sccache' >> ~/.bashrc

echo "=== Setup complete ==="
echo "Run 'source ~/.bashrc' or start a new shell to use sccache"
