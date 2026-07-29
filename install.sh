#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
UUID="soundblaster-g6-output@netherg.io"
BIN_DIR="${HOME}/.local/bin"
EXTENSION_DIR="${HOME}/.local/share/gnome-shell/extensions/${UUID}"
RULE_SOURCE="${ROOT_DIR}/packaging/70-soundblaster-g6-output.rules"
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
        log "sudo is required to install the udev rule"
        exit 1
    fi
}

find_binary() {
    if [[ -x "${ROOT_DIR}/bin/g6-output" ]]; then
        printf '%s\n' "${ROOT_DIR}/bin/g6-output"
        return
    fi

    if [[ -x "${ROOT_DIR}/target/release/g6-output" ]]; then
        printf '%s\n' "${ROOT_DIR}/target/release/g6-output"
        return
    fi

    if command -v cargo >/dev/null 2>&1; then
        log "Building the Rust binary..." >&2
        # This function returns its result on stdout, so keep the build's own
        # output off it entirely.
        (cd "${ROOT_DIR}" && cargo build --release >&2)
        printf '%s\n' "${ROOT_DIR}/target/release/g6-output"
        return
    fi

    log "No prebuilt binary found and Cargo is not installed" >&2
    log "Download the release tarball or install Rust to build from source" >&2
    exit 1
}

BINARY_SOURCE="$(find_binary)"

# The udev rule goes first because it is the only step needing root: if the
# user declines the password prompt, nothing has been installed yet. Doing it
# last would leave a CLI and an extension in place that fail on every switch.
log "Installing udev rule"
run_as_root install -D -m 0644 "${RULE_SOURCE}" "${RULE_DEST}"
run_as_root udevadm control --reload-rules
run_as_root udevadm trigger --subsystem-match=hidraw || true

log "Installing CLI to ${BIN_DIR}/g6-output"
install -d -m 0755 "${BIN_DIR}"
# Install to a temporary name and rename, so reinstalling while g6-output is
# running does not fail with ETXTBSY.
install -m 0755 "${BINARY_SOURCE}" "${BIN_DIR}/.g6-output.new"
mv -f "${BIN_DIR}/.g6-output.new" "${BIN_DIR}/g6-output"

log "Installing GNOME extension"
rm -rf "${EXTENSION_DIR}"
install -d -m 0755 "${EXTENSION_DIR}"
cp -a "${ROOT_DIR}/extension/." "${EXTENSION_DIR}/"

case ":${PATH}:" in
    *":${BIN_DIR}:"*) ;;
    *) log "WARNING: ${BIN_DIR} is not on your PATH; call the binary by full path" ;;
esac

if command -v gnome-extensions >/dev/null 2>&1; then
    if gnome-extensions enable "${UUID}" 2>/dev/null; then
        log "GNOME extension enabled"
    else
        log "GNOME Shell has not discovered the extension yet"
    fi
fi

cat <<EOF_MESSAGE

Installed successfully.

1. Unplug and reconnect the Sound BlasterX G6.
2. Log out and back in if the panel icon does not appear.
3. If needed, enable the extension manually:
   gnome-extensions enable ${UUID}
4. Check device access:
   g6-output doctor

Remove everything with:
   ${ROOT_DIR}/uninstall.sh
EOF_MESSAGE
