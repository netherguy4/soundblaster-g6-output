#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${VERSION:-0.1.0-dev}"
ARCH="${ARCH:-$(uname -m)}"
PACKAGE_NAME="soundblaster-g6-output-${VERSION}-${ARCH}"
STAGE_DIR="${ROOT_DIR}/dist/${PACKAGE_NAME}"
BINARY="${ROOT_DIR}/target/release/g6-output"

if ! command -v zip >/dev/null 2>&1; then
    printf 'zip is required to package the GNOME extension\n' >&2
    exit 1
fi

if [[ ! -x "${BINARY}" ]]; then
    printf 'Missing %s — run "cargo build --release" first\n' "${BINARY}" >&2
    exit 1
fi

# A tag like v0.2.0 packaged against a Cargo.toml still saying 0.1.0 would ship
# an archive whose name and `--version` output disagree.
CRATE_VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "${ROOT_DIR}/Cargo.toml" | head -n 1)"
case "${VERSION}" in
    # Exact match for releases; "<crate>-suffix" for local test builds; "dev-<sha>"
    # for CI runs on untagged commits.
    "${CRATE_VERSION}"|"${CRATE_VERSION}-"*|dev-*) ;;
    *)
        printf 'VERSION=%s does not match Cargo.toml version %s\n' "${VERSION}" "${CRATE_VERSION}" >&2
        exit 1
        ;;
esac

rm -rf "${ROOT_DIR}/dist"
mkdir -p "${STAGE_DIR}/bin" "${STAGE_DIR}/extension" "${STAGE_DIR}/packaging" "${STAGE_DIR}/docs/handoff"

install -m 0755 "${BINARY}" "${STAGE_DIR}/bin/g6-output"
cp -a "${ROOT_DIR}/extension/." "${STAGE_DIR}/extension/"
cp -a "${ROOT_DIR}/packaging/." "${STAGE_DIR}/packaging/"
install -m 0755 "${ROOT_DIR}/install.sh" "${STAGE_DIR}/install.sh"
install -m 0755 "${ROOT_DIR}/uninstall.sh" "${STAGE_DIR}/uninstall.sh"
install -m 0644 "${ROOT_DIR}/README.md" "${STAGE_DIR}/README.md"
install -m 0644 "${ROOT_DIR}/HANDOFF.md" "${STAGE_DIR}/HANDOFF.md"
cp -a "${ROOT_DIR}/docs/handoff/." "${STAGE_DIR}/docs/handoff/"
install -m 0644 "${ROOT_DIR}/LICENSE" "${STAGE_DIR}/LICENSE"
install -m 0644 "${ROOT_DIR}/NOTICE.md" "${STAGE_DIR}/NOTICE.md"

(
    cd "${ROOT_DIR}/dist"
    tar -czf "${PACKAGE_NAME}.tar.gz" "${PACKAGE_NAME}"
)

(
    cd "${ROOT_DIR}/extension"
    zip -qr "${ROOT_DIR}/dist/soundblaster-g6-output@netherg.io.zip" .
)

(
    cd "${ROOT_DIR}/dist"
    sha256sum "${PACKAGE_NAME}.tar.gz" "soundblaster-g6-output@netherg.io.zip" > SHA256SUMS
)

printf 'Created release artifacts in %s/dist\n' "${ROOT_DIR}"
