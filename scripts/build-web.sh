#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
./scripts/package-notices.sh
rm -rf web/pkg web/pkg-grammars
cargo build -p markit-wasm --release --locked --target wasm32-unknown-unknown
wasm-bindgen target/wasm32-unknown-unknown/release/markit_wasm.wasm --target web --out-dir web/pkg
# Grammar syntaxes are opt-in and loaded only by LineHighlighter or grammar highlighting.
cargo build -p markit-wasm --release --locked --features grammars --target wasm32-unknown-unknown
wasm-bindgen target/wasm32-unknown-unknown/release/markit_wasm.wasm --target web --out-dir web/pkg-grammars
