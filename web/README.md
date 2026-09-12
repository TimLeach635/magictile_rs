# The tiling viewer on the web

`build.sh` produces a self-contained static site in `web/dist`: an HTML page, a JavaScript loader
and a WebAssembly module. Nothing is fetched from anywhere else, so any static web server will do.

It builds the tiling viewer only, not the puzzle app, so there are no embedded puzzle configs, no
threads and no randomness in the module.

## Building on Debian

Debian's packaged `rustc` is usually too old: the code needs Rust 1.88 or newer (edition 2024 and
let-chains). Install the toolchain with rustup:

    sudo apt update
    sudo apt install -y build-essential curl git brotli
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    . "$HOME/.cargo/env"
    rustup target add wasm32-unknown-unknown

Then the wasm-bindgen CLI, at exactly the version in `Cargo.lock` (`build.sh` checks, and prints
this command if it disagrees):

    cargo install wasm-bindgen-cli --locked --version "$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/[",]/, "", $3); print $3; exit }' Cargo.lock)"

No graphics packages are needed: nothing native is compiled, only WebAssembly.

    ./web/build.sh

Building wgpu takes a couple of gigabytes of memory. On a small VPS it may be killed part way; add
swap, or build on your own machine and copy the result up:

    rsync -av --delete web/dist/ user@your-vps:/var/www/tiling/

## Serving it

The page has to come over http(s) — opening `index.html` from disk won't work, as browsers refuse
to load WebAssembly modules from `file://`. To check a build locally:

    python3 -m http.server -d web/dist 8080

### Caddy

    tiling.example.com {
        root * /var/www/tiling
        encode zstd gzip
        file_server
    }

### nginx

    server {
        listen 80;
        server_name tiling.example.com;
        root /var/www/tiling;

        # Hand over the .gz and .br files that build.sh made, rather than compressing each request.
        gzip_static on;
        # brotli_static on;   # only if your nginx has the brotli module

        location / {
            try_files $uri $uri/ =404;
        }
    }

Browsers need the module served as `application/wasm`. Current nginx knows that already; check with
`grep wasm /etc/nginx/mime.types` and, if it is missing, add `application/wasm wasm;` to that file.
Do not add a `types { ... }` block to the server or location instead: that replaces the whole map
rather than adding to it.

## Showing a different tiling

`index.html` ends with `start(canvas, 4, 5)`, which is t{4,5}, the truncated order-5 square tiling.
Other hyperbolic pairs work too, so long as (p-2)(q-2) > 4 — `start(canvas, 3, 7)` for instance.
The permutation labels only appear when the tiling can carry them: q - 1 has to divide p.
