#!/bin/bash

# Nori CLI Notification Hook
# This script receives notification JSON as a CLI argument and displays OS-level notifications.
# Supports Linux (notify-send), macOS (osascript), and Windows (PowerShell).

set -e

# Configuration
readonly NOTIFICATION_TITLE="Nori"
readonly LOG_FILE="${NORI_NOTIFY_LOG:-/tmp/nori-notify.log}"

# Logging function
log() {
    if [[ -n "$NORI_NOTIFY_DEBUG" ]]; then
        echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*" >> "$LOG_FILE"
    fi
}

# Get JSON from CLI argument
NOTIFICATION_JSON="${1:-}"
if [[ -z "$NOTIFICATION_JSON" ]]; then
    log "ERROR: No notification JSON provided"
    exit 0  # Exit gracefully to avoid blocking the CLI
fi

log "Received notification: $NOTIFICATION_JSON"

# Parse message from JSON using available tools
parse_message() {
    local json="$1"
    local message=""

    # Try python3 first (most reliable)
    if command -v python3 >/dev/null 2>&1; then
        message=$(echo "$json" | python3 -c "
import sys, json
try:
    data = json.loads(sys.stdin.read())
    msg_type = data.get('type', '')
    if msg_type == 'approval-requested':
        req_type = data.get('request-type', 'action')
        msg = data.get('message', 'Approval needed')
        idle = data.get('idle-duration-secs')
        if idle:
            print(f'{msg} (idle {idle}s)')
        else:
            print(msg)
    elif msg_type == 'agent-turn-complete':
        print(data.get('last-assistant-message', 'Agent completed')[:100])
    else:
        print(data.get('message', 'Notification'))
except Exception as e:
    print('Nori needs your attention')
" 2>/dev/null)
    fi

    # Try jq if python3 failed
    if [[ -z "$message" ]] && command -v jq >/dev/null 2>&1; then
        message=$(echo "$json" | jq -r '.message // "Nori needs your attention"' 2>/dev/null)
    fi

    # Fallback
    if [[ -z "$message" ]]; then
        message="Nori needs your attention"
    fi

    echo "$message"
}

MESSAGE=$(parse_message "$NOTIFICATION_JSON")
log "Parsed message: $MESSAGE"

# Detect OS
OS_TYPE=$(uname -s)

# Send notification based on OS
case "$OS_TYPE" in
    Linux*)
        if command -v notify-send >/dev/null 2>&1; then
            notify-send "$NOTIFICATION_TITLE" "$MESSAGE" --icon=dialog-information --urgency=normal 2>/dev/null || true
            log "Sent Linux notification via notify-send"
        else
            log "WARNING: notify-send not found. Install libnotify-bin for notifications."
        fi
        ;;
    Darwin*)
        # Escape message for AppleScript (escape backslashes and quotes)
        ESCAPED_MESSAGE=$(printf '%s' "$MESSAGE" | sed 's/\\/\\\\/g; s/"/\\"/g')
        # Try terminal-notifier first (better click-to-focus support)
        if command -v terminal-notifier >/dev/null 2>&1; then
            terminal-notifier -title "$NOTIFICATION_TITLE" -message "$ESCAPED_MESSAGE" -sound default 2>/dev/null || true
            log "Sent macOS notification via terminal-notifier"
        else
            # Fallback to osascript
            osascript -e "display notification \"$ESCAPED_MESSAGE\" with title \"$NOTIFICATION_TITLE\"" 2>/dev/null || true
            log "Sent macOS notification via osascript"
        fi
        ;;
    MINGW*|MSYS*|CYGWIN*)
        # Windows via PowerShell
        if command -v powershell.exe >/dev/null 2>&1; then
            # Escape message for PowerShell (escape single quotes by doubling)
            PS_MESSAGE=$(printf '%s' "$MESSAGE" | sed "s/'/''/g")
            powershell.exe -Command "
                Add-Type -AssemblyName System.Windows.Forms
                \$notify = New-Object System.Windows.Forms.NotifyIcon
                \$notify.Icon = [System.Drawing.SystemIcons]::Information
                \$notify.Visible = \$true
                \$notify.ShowBalloonTip(5000, '$NOTIFICATION_TITLE', '$PS_MESSAGE', 'Info')
                Start-Sleep -Seconds 1
                \$notify.Dispose()
            " 2>/dev/null || true
            log "Sent Windows notification via PowerShell"
        fi
        ;;
    *)
        log "WARNING: Unsupported OS: $OS_TYPE"
        ;;
esac

log "Notification hook completed"
exit 0
