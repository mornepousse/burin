#!/usr/bin/env bash
# Makes dist-web/: the web version, a page and its files to put on any web server as they are.
# Needs only Nix with flakes. The tools stay out of the system, in /tmp/burin-release (or BURIN_RELEASE_TOOLS).
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")/.."

RUST=1.95.0
# The program that finishes the build must be the version of the library the program uses.
BINDGEN=$(sed -n '/^name = "wasm-bindgen"$/{n;s/^version = "\(.*\)"/\1/p}' Cargo.lock)

if [ "${1:-}" != --inside ]; then
    exec nix shell nixpkgs#rustup nixpkgs#lld nixpkgs#binaryen -c bash "$0" --inside
fi

TOOLS=${BURIN_RELEASE_TOOLS:-/tmp/burin-release}
mkdir -p "$TOOLS"
export RUSTUP_HOME=$TOOLS/rustup XDG_CACHE_HOME=$TOOLS/cache CARGO_TARGET_DIR=$TOOLS/target
export CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix=$HOME=/build"$'\x1f'"--remap-path-prefix=$TOOLS/source-web=/build/burin"
unset LD_LIBRARY_PATH

rustup toolchain list | grep -q "^$RUST" || rustup toolchain install "$RUST" --profile minimal --no-self-update
rustup target list --installed --toolchain "$RUST" | grep -q wasm32-unknown-unknown || rustup target add --toolchain "$RUST" wasm32-unknown-unknown
WASM_TOOLS=$TOOLS/wasm-tools
if ! "$WASM_TOOLS/bin/wasm-bindgen" --version 2>/dev/null | grep -qF "$BINDGEN"; then
    CARGO_TARGET_DIR=$WASM_TOOLS/target rustup run "$RUST" cargo install wasm-bindgen-cli --version "$BINDGEN" --locked --root "$WASM_TOOLS"
fi

# Built from a copy in a neutral place, as the other programs are.
SOURCE=$TOOLS/source-web
rm -rf "$SOURCE"
mkdir -p "$SOURCE"
cp -a Cargo.toml Cargo.lock build.rs src release assets "$SOURCE/"
(cd "$SOURCE" && rustup run "$RUST" cargo build --release --locked --target wasm32-unknown-unknown)

OUT=dist-web
rm -rf "$OUT"
mkdir -p "$OUT"
"$WASM_TOOLS/bin/wasm-bindgen" --target web --no-typescript --out-dir "$OUT" --out-name burin "$CARGO_TARGET_DIR/wasm32-unknown-unknown/release/burin.wasm"
# Smaller to download; the same program.
wasm-opt -Os --strip-debug --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext --enable-mutable-globals -o "$OUT/burin_bg.wasm" "$OUT/burin_bg.wasm"
# The build's mark, from the program itself: a new build is a new address for both halves.
BUILD=$(sha256sum "$OUT/burin_bg.wasm" | cut -c1-12)
sed "s/{{BUILD}}/$BUILD/g" release/web/index.html > "$OUT/index.html"
cp release/burin.png "$OUT/"

if strings -a "$OUT/burin_bg.wasm" | grep -F -e "$HOME" -e "$PWD" | head -n 3 | grep . ; then
    echo "Paths on this computer are still in the web version" >&2
    exit 1
fi
ls -la "$OUT"
