#!/usr/bin/env bash
# Builds the wasm module and puts it next to the page.
#
# There is deliberately no npm, no bundler and no bindings generator here: the
# module exports a plain C ABI, so cargo alone is the whole toolchain.
set -euo pipefail

cd "$(dirname "$0")"
PROFILE="${1:-release}"
TARGET=wasm32-unknown-unknown

if ! rustup target list --installed | grep -qx "$TARGET"; then
    echo "adding the $TARGET target"
    rustup target add "$TARGET"
fi

if [ "$PROFILE" = "debug" ]; then
    cargo build --target "$TARGET" -p zelduh-wasm
    OUT="target/$TARGET/debug/zelduh_wasm.wasm"
else
    cargo build --release --target "$TARGET" -p zelduh-wasm
    OUT="target/$TARGET/release/zelduh_wasm.wasm"
fi

cp "$OUT" web/zelduh.wasm
printf 'built web/zelduh.wasm (%s KiB)\n' "$(( $(wc -c < web/zelduh.wasm) / 1024 ))"
echo
echo 'to play:  python3 -m http.server -d web 8080   then open http://localhost:8080'
