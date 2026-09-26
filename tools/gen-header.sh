#!/usr/bin/env bash
# Regenerates the committed C header from medius-capi; run after a C ABI change.
# CI runs it and fails on drift (`git diff --exit-code`).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out="$here/medius-capi/include/medius.h"

mkdir -p "$(dirname "$out")"
cbindgen \
    --config "$here/medius-capi/cbindgen.toml" \
    --crate medius-capi \
    --output "$out"

echo "wrote $out"
