#!/usr/bin/env bash
# Regenerate the committed C header from medius-capi. Run after changing the C ABI.
# CI runs this and checks `git diff --exit-code` to catch drift.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out="$here/medius-capi/include/medius.h"

mkdir -p "$(dirname "$out")"
cbindgen \
    --config "$here/medius-capi/cbindgen.toml" \
    --crate medius-capi \
    --output "$out"

echo "wrote $out"
