#!/bin/bash

# Cloud Agent Setup Script
# ========================
# Sets up the nori-skillsets coding agent environment for cloud-hosted
# Claude Code environments (e.g., Claude Code web).
#
# This script:
#   1. Checks if running in a Claude web environment (exits early if not)
#   2. Checks if nori-skillsets is already available
#   3. Determines the best JS package manager (bun preferred, npm fallback)
#   4. Installs nori-skillsets globally, or falls back to npx/bunx if permissions fail
#   5. Runs nori-skillsets install <skillset> from the repo root
#
# Usage:
#   ./scripts/cloud_agent_setup.sh [skillset]
#
#   skillset: Optional. The skillset to install (default: senior-swe)
#
# Requirements:
#   - Must be run from within a git repository
#   - Either bun or npm must be available
#   - Must be running in a Claude web environment

# Exit early if not in Claude web environment
is_claude_web_env() {
    # Environment indicators (most reliable)
    [[ "$CLAUDE_CODE_REMOTE" == "true" ]] || return 1
    [[ "$CLAUDE_CODE_REMOTE_ENVIRONMENT_TYPE" == "cloud_default" ]] || return 1
    [[ "$CLAUDE_CODE_ENTRYPOINT" == "remote" ]] || return 1

    return 0
}

if ! is_claude_web_env; then
    echo "Not a Claude web environment, skipping cloud setup"
    exit 0
fi

set -e
set -u
set -o pipefail

# Parse arguments
SKILLSET="${1:-senior-swe}"

# Logging functions (no color - cloud environments may not support ANSI escape codes)
log_info() {
    echo "==> $1"
}

log_success() {
    echo "✓ $1"
}

log_error() {
    echo "✗ $1" >&2
}

# Detect repo root
log_info "Detecting repository root..."
if ! REPO_ROOT=$(git rev-parse --show-toplevel 2>/dev/null); then
    log_error "Not inside a git repository"
    exit 1
fi
log_success "Repository root: $REPO_ROOT"

# Check connectivity to noriskillsets.dev registry
log_info "Checking connectivity to noriskillsets.dev..."
if ! curl -sf --connect-timeout 5 https://noriskillsets.dev/health > /dev/null 2>&1; then
    echo ""
    echo "╔══════════════════════════════════════════════════════════════════════════════╗"
    echo "║                                                                              ║"
    echo "║   ERROR: Cannot connect to noriskillsets.dev                                 ║"
    echo "║                                                                              ║"
    echo "║   This is required to download skillsets from the Nori registry.             ║"
    echo "║                                                                              ║"
    echo "║   If you're using Claude Code on the web, add these domains to your          ║"
    echo "║   organization's custom allowed domains list:                                ║"
    echo "║                                                                              ║"
    echo "║       noriskillsets.dev                                                      ║"
    echo "║       *.noriskillsets.dev                                                    ║"
    echo "║                                                                              ║"
    echo "║   Then RESTART this session for the changes to take effect.                  ║"
    echo "║                                                                              ║"
    echo "║   To configure allowed domains:                                              ║"
    echo "║   1. Go to your Claude organization settings                                 ║"
    echo "║   2. Navigate to 'Claude Code' or 'Network Access' settings                  ║"
    echo "║   3. Add the domains above to the custom allowed domains list                ║"
    echo "║   4. Start a new Claude Code session                                         ║"
    echo "║                                                                              ║"
    echo "╚══════════════════════════════════════════════════════════════════════════════╝"
    echo ""
    exit 1
fi
log_success "Registry is reachable"

# Check if nori-skillsets is already available
log_info "Checking if nori-skillsets is available..."
if command -v nori-skillsets &> /dev/null; then
    log_success "nori-skillsets is already installed"
    NORI_CMD="nori-skillsets"
else
    log_info "nori-skillsets not found, will install nori-skillsets package"

    # Determine the best JS package manager
    log_info "Detecting JavaScript package manager..."
    PKG_MANAGER=""
    RUNNER=""

    if command -v bun &> /dev/null; then
        PKG_MANAGER="bun"
        RUNNER="bunx"
        log_success "Found bun"
    elif command -v npm &> /dev/null; then
        PKG_MANAGER="npm"
        RUNNER="npx --yes"
        log_success "Found npm"
    else
        log_error "Neither bun nor npm is available"
        log_error "Please install Node.js (npm) or bun first"
        exit 1
    fi

    # Attempt global install
    log_info "Attempting global install of nori-skillsets using $PKG_MANAGER..."
    INSTALL_SUCCESS=false

    if [ "$PKG_MANAGER" = "bun" ]; then
        if bun install -g nori-skillsets 2>/dev/null; then
            INSTALL_SUCCESS=true
        fi
    else
        if npm install -g nori-skillsets 2>/dev/null; then
            INSTALL_SUCCESS=true
        fi
    fi

    if [ "$INSTALL_SUCCESS" = true ]; then
        log_success "nori-skillsets installed globally"
        NORI_CMD="nori-skillsets"
    else
        log_info "Global install failed (likely permissions), will use $RUNNER instead"
        NORI_CMD="$RUNNER nori-skillsets"
    fi
fi

# Change to repo root
log_info "Changing to repository root..."
cd "$REPO_ROOT"
log_success "Working directory: $(pwd)"

# Run nori-skillsets install <skillset>
log_info "Running: $NORI_CMD install $SKILLSET"
$NORI_CMD install "$SKILLSET"

log_success "Cloud agent setup complete!"

# Prepend verification message to AGENTS.md (for hook verification)
AGENTS_FILE="$REPO_ROOT/AGENTS.md"
if [ -f "$AGENTS_FILE" ]; then
    VERIFICATION_MSG='> "IMPORTANT: Always announce at the beginning of your session, that you are operating in a web environment, and will make best attempts to setup Nori tooling"'
    # Only prepend if not already present
    if ! grep -qF "$VERIFICATION_MSG" "$AGENTS_FILE"; then
        log_info "Adding verification message to AGENTS.md..."
        echo -e "$VERIFICATION_MSG\n\n$(cat "$AGENTS_FILE")" > "$AGENTS_FILE"
        log_success "Verification message added to AGENTS.md"
    fi
fi

echo ""
echo "Next steps:"
echo "  1. Restart Claude Code to apply the new profile"
echo "  2. The $SKILLSET skillset is now active"
