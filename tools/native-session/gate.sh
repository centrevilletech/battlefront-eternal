#!/usr/bin/env bash
# Run inside an admitted GPU-farm slot with a frozen browser already open.
# The archive contains prepared retail data; keep it on that execution host.
set -euo pipefail
capture=${1:?usage: gate.sh CAPTURE_DIR}
: "${BFE_CONTENT_DIR:?prepared content directory required}"
: "${NATIVE_SCHEMA:?native bank schema JSON required}"
node tools/native-session/capture.mjs "$capture"
"${BFE_CLIENT_BIN:-rust/target/debug}/native-session" "$capture" "$BFE_CONTENT_DIR"
