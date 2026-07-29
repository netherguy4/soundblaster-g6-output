#!/usr/bin/env bash
set -euo pipefail

UUID="soundblaster-g6-output@netherg.io"
BINARY="${HOME}/.local/bin/g6-output"
EXTENSION_DIR="${HOME}/.local/share/gnome-shell/extensions/${UUID}"
STATE_DIR="${XDG_STATE_HOME:-${HOME}/.local/state}/g6-output"
RULE_DEST="/etc/udev/rules.d/70-soundblaster-g6-output.rules"

log() {
    printf '[g6-output] %s\n' "$*"
}

run_as_root() {
    if [[ ${EUID} -eq 0 ]]; then
        "$@"
    elif command -v sudo >/dev/null 2>&1; then
        sudo "$@"
    else
        log "sudo is required to remove the udev rule"
        exit 1
    fi
}

if command -v gnome-extensions >/dev/null 2>&1; then
    gnome-extensions disable "${UUID}" 2>/dev/null || true
fi

log "Removing GNOME extension"
rm -rf "${EXTENSION_DIR}"

log "Removing CLI and state"
rm -f "${BINARY}"
# Leftover from an install.sh interrupted between install and rename.
rm -f "$(dirname -- "${BINARY}")/.g6-output.new"
rm -rf "${STATE_DIR}"

if [[ -e "${RULE_DEST}" ]]; then
    log "Removing udev rule"
    run_as_root rm -f "${RULE_DEST}"
    run_as_root udevadm control --reload-rules
    run_as_root udevadm trigger --subsystem-match=hidraw || true
fi

cat <<'EOF_MESSAGE'

Removed successfully.
Log out and back in if the panel icon remains visible.
EOF_MESSAGE
