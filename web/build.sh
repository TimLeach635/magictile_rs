#!/bin/sh
# Builds the tiling viewer for the browser into web/dist.
#
# Needs a recent stable Rust, the wasm32-unknown-unknown target, and wasm-bindgen-cli at the
# version in Cargo.lock. See web/README.md for setting that up on a server.
set -eu

cd "$(dirname "$0")/.."

# The wasm-bindgen CLI has to match the crate version exactly; a mismatch produces a bundle the
# browser rejects, so check before building rather than after.
wanted=$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/[",]/, "", $3); print $3; exit }' Cargo.lock)
have=$(wasm-bindgen --version 2>/dev/null | cut -d' ' -f2 || true)
if [ "$have" != "$wanted" ]; then
    echo "This needs wasm-bindgen $wanted (found: ${have:-none})." >&2
    echo "Install it with: cargo install wasm-bindgen-cli --locked --version $wanted" >&2
    exit 1
fi

# Start from an empty directory, so nothing stale is left behind to be deployed.
rm -rf web/dist

# Only the viewer: no puzzle app, so no embedded configs, threads or randomness.
cargo build --release --locked --target wasm32-unknown-unknown -p magictile --lib --no-default-features
wasm-bindgen --target web --no-typescript --out-dir web/dist \
    target/wasm32-unknown-unknown/release/magictile.wasm
cp web/index.html web/dist/

# Precompress, so a static server can hand these over without compressing on every request.
for file in web/dist/magictile_bg.wasm web/dist/magictile.js; do
    gzip -9 -c "$file" > "$file.gz"
    if command -v brotli > /dev/null; then
        brotli -f -q 11 "$file"
    fi
done

echo "Built web/dist:"
ls -1sh web/dist
