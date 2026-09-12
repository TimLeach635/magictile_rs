#!/bin/sh
# Builds the tiling viewer for the browser into web/dist.
set -e
cd "$(dirname "$0")/.."
cargo build --release --target wasm32-unknown-unknown -p magictile --lib --no-default-features
wasm-bindgen --target web --no-typescript --out-dir web/dist \
  target/wasm32-unknown-unknown/release/magictile.wasm
cp web/index.html web/dist/
echo "Built web/dist. Serve it with: python3 -m http.server -d web/dist 8080"
