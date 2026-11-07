#!/usr/bin/env bash
# Makes dist/Burin-<version>-x86_64.AppImage and dist/Burin-<version>-windows-x86_64.exe.
# Needs only Nix with flakes. The tools stay out of the system: Nix keeps them in its store,
# and the rest goes in /tmp/burin-release (or BURIN_RELEASE_TOOLS).
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")/.."

RUST=1.95.0
GLIBC=2.31 # The oldest Linux it should run on.
RUNTIME_VERSION=20251108
RUNTIME_SHA256=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d

if [ "${1:-}" != --inside ]; then
    exec nix shell nixpkgs#rustup nixpkgs#zig nixpkgs#cargo-zigbuild nixpkgs#squashfsTools \
        nixpkgs#pkgsCross.mingwW64.buildPackages.binutils nixpkgs#pkgsCross.mingwW64.stdenv.cc \
        -c bash "$0" --inside
fi

TOOLS=${BURIN_RELEASE_TOOLS:-/tmp/burin-release}
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
mkdir -p "$TOOLS"
export RUSTUP_HOME=$TOOLS/rustup XDG_CACHE_HOME=$TOOLS/cache
# No spaces here: the Windows tools can't take a folder name that has them.
export CARGO_TARGET_DIR=$TOOLS/target
# Paths on this computer stay out of the programs.
export CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix=$PWD=/build/burin"$'\x1f'"--remap-path-prefix=$HOME=/build"
unset LD_LIBRARY_PATH

rustup toolchain list | grep -q "^$RUST" || rustup toolchain install "$RUST" --profile minimal --no-self-update
rustup target list --installed --toolchain "$RUST" | grep -q x86_64-pc-windows-gnu || rustup target add --toolchain "$RUST" x86_64-pc-windows-gnu
rustup run "$RUST" cargo zigbuild --release --locked --target "x86_64-unknown-linux-gnu.$GLIBC"
rustup run "$RUST" cargo zigbuild --release --locked --target x86_64-pc-windows-gnu

rm -rf dist
mkdir -p dist

# A list of what's inside and under which licence, to go with the programs.
rustup run "$RUST" cargo metadata --locked --format-version 1 --filter-platform x86_64-unknown-linux-gnu --filter-platform x86_64-pc-windows-gnu \
    | python3 -I release/third_party.py > dist/THIRD-PARTY.txt

cp "$CARGO_TARGET_DIR/x86_64-pc-windows-gnu/release/burin.exe" "dist/Burin-$VERSION-windows-x86_64.exe"

# An AppImage is a small starter program with a compressed folder right after it.
RUNTIME=$TOOLS/runtime/runtime-x86_64
if [ ! -f "$RUNTIME" ]; then
    mkdir -p "$TOOLS/runtime"
    curl -fsSL -o "$RUNTIME" "https://github.com/AppImage/type2-runtime/releases/download/$RUNTIME_VERSION/runtime-x86_64"
fi
echo "$RUNTIME_SHA256  $RUNTIME" | sha256sum -c --quiet

APPDIR=$TOOLS/Burin.AppDir
rm -rf "$APPDIR" "$TOOLS/burin.squashfs"
mkdir -p "$APPDIR/usr/bin"
cp "$CARGO_TARGET_DIR/x86_64-unknown-linux-gnu/release/burin" "$APPDIR/usr/bin/burin"
cp release/burin.desktop release/burin.png dist/THIRD-PARTY.txt "$APPDIR/"
cp release/burin.png "$APPDIR/.DirIcon"
cat > "$APPDIR/AppRun" <<'RUN'
#!/bin/sh
exec "$(dirname "$(readlink -f "$0")")/usr/bin/burin" "$@"
RUN
chmod +x "$APPDIR/AppRun"
mksquashfs "$APPDIR" "$TOOLS/burin.squashfs" -root-owned -noappend -comp zstd -quiet -no-progress
cat "$RUNTIME" "$TOOLS/burin.squashfs" > "dist/Burin-$VERSION-x86_64.AppImage"
chmod +x "dist/Burin-$VERSION-x86_64.AppImage"

(cd dist && sha256sum Burin-* > SHA256SUMS)
ls -la dist
