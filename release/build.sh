#!/usr/bin/env bash
# Makes dist/Burin-<version>-x86_64.AppImage, dist/Burin-<version>-windows-x86_64.exe, and the
# packages for Debian/Ubuntu (.deb) and Fedora/openSUSE (.rpm).
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
        nixpkgs#pkgsCross.mingwW64.buildPackages.binutils nixpkgs#pkgsCross.mingwW64.stdenv.cc nixpkgs#nfpm \
        -c bash "$0" --inside
fi

TOOLS=${BURIN_RELEASE_TOOLS:-/tmp/burin-release}
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
mkdir -p "$TOOLS"
export RUSTUP_HOME=$TOOLS/rustup XDG_CACHE_HOME=$TOOLS/cache
# No spaces here: the Windows tools can't take a folder name that has them.
export CARGO_TARGET_DIR=$TOOLS/target
# Paths on this computer stay out of the programs.
# The last that fits is the one used, so the more exact one comes last.
export CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix=$HOME=/build"$'\x1f'"--remap-path-prefix=$TOOLS/source=/build/burin"
unset LD_LIBRARY_PATH
# Where a report's draft opens: the project's page, if it's kept on GitHub. Nothing else from the address.
ORIGIN=$(git remote get-url origin 2>/dev/null || true)
OWNER_REPO=$(printf '%s' "$ORIGIN" | sed -nE 's#^(https://([^@/]*@)?github\.com/|git@github\.com:)([A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)$#\3#p' | sed 's/\.git$//')
if [ -n "$OWNER_REPO" ]; then
    export BURIN_ISSUES_URL="https://github.com/$OWNER_REPO/issues/new"
fi
# Problem reports go through the relay (relay/): no address is written into the programs.
unset BURIN_REPORT_EMAIL

rustup toolchain list | grep -q "^$RUST" || rustup toolchain install "$RUST" --profile minimal --no-self-update
rustup target list --installed --toolchain "$RUST" | grep -q x86_64-pc-windows-gnu || rustup target add --toolchain "$RUST" x86_64-pc-windows-gnu
# Built from a copy in a neutral place: the folder a program is built in ends up inside it.
SOURCE=$TOOLS/source
rm -rf "$SOURCE"
mkdir -p "$SOURCE"
cp -a Cargo.toml Cargo.lock build.rs src release assets "$SOURCE/"
(
    cd "$SOURCE"
    rustup run "$RUST" cargo zigbuild --release --locked --target "x86_64-unknown-linux-gnu.$GLIBC"
    rustup run "$RUST" cargo zigbuild --release --locked --target x86_64-pc-windows-gnu
)

# Stops here rather than hand out a program that says where it was built, or whose home that was.
for PROGRAM in "$CARGO_TARGET_DIR/x86_64-pc-windows-gnu/release/burin.exe" "$CARGO_TARGET_DIR/x86_64-unknown-linux-gnu/release/burin"; do
    if strings -a "$PROGRAM" | grep -F -e "$HOME" -e "$PWD" | head -n 3 | grep . ; then
        echo "Paths on this computer are still in $PROGRAM" >&2
        exit 1
    fi
done

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

# The same program as a package the system installs: in the applications menu, opening .burin
# files, removed like any other. What draws the window is found at run time, so it is only
# recommended; curl is for the update check and problem reports.
PKGROOT=$TOOLS/package
rm -rf "$PKGROOT"
mkdir -p "$PKGROOT/applications" "$PKGROOT/mime"
sed -e 's/^Exec=burin$/Exec=burin %f/' release/burin.desktop > "$PKGROOT/applications/burin.desktop"
echo "MimeType=application/x-burin;" >> "$PKGROOT/applications/burin.desktop"
cat > "$PKGROOT/mime/burin.xml" <<'MIME'
<?xml version="1.0" encoding="UTF-8"?>
<mime-info xmlns="http://www.freedesktop.org/standards/shared-mime-info">
  <mime-type type="application/x-burin">
    <comment>Burin project</comment>
    <glob pattern="*.burin"/>
  </mime-type>
</mime-info>
MIME
cat > "$PKGROOT/nfpm.yaml" <<YAML
name: burin
arch: amd64
platform: linux
version: "$VERSION"
release: 1
section: graphics
priority: optional
maintainer: "Burin <burin@example.invalid>"
description: |
  Draw, model and cut for CNC carving.
  Burin draws parts, prepares the toolpaths (profiles, pockets, drilling, V-carving, 3D)
  and writes the file the CNC machine reads.
homepage: "https://burin.cc"
license: GPL-3.0-or-later
depends:
  - libc6 (>= $GLIBC)
recommends:
  - libgl1
  - libvulkan1
  - libxkbcommon0
  - libwayland-client0
  - libxcursor1
  - libxi6
  - libxrandr2
  - curl
rpm:
  # Not this computer's name.
  buildhost: build
overrides:
  rpm:
    depends:
      - glibc >= $GLIBC
    recommends:
      - mesa-libGL
      - vulkan-loader
      - libxkbcommon
      - libwayland-client
      - libXcursor
      - libXi
      - libXrandr
      - curl
contents:
  - src: $CARGO_TARGET_DIR/x86_64-unknown-linux-gnu/release/burin
    dst: /usr/bin/burin
    file_info: { mode: 0755 }
  - src: $PKGROOT/applications/burin.desktop
    dst: /usr/share/applications/burin.desktop
  - src: $PWD/release/burin.png
    dst: /usr/share/icons/hicolor/512x512/apps/burin.png
  - src: $PKGROOT/mime/burin.xml
    dst: /usr/share/mime/packages/burin.xml
  - src: $PWD/dist/THIRD-PARTY.txt
    dst: /usr/share/doc/burin/THIRD-PARTY.txt
  - src: $PWD/LICENSE
    dst: /usr/share/doc/burin/LICENSE
YAML
nfpm package --config "$PKGROOT/nfpm.yaml" --packager deb --target "dist/Burin-$VERSION-amd64.deb"
nfpm package --config "$PKGROOT/nfpm.yaml" --packager rpm --target "dist/Burin-$VERSION-x86_64.rpm"
# The packages' headers are plain text: this computer's name and home must not be in them. Only
# the headers are read (the compressed part could hold those letters by chance).
for PACKAGE in "dist/Burin-$VERSION-amd64.deb" "dist/Burin-$VERSION-x86_64.rpm"; do
    if head -c 16384 "$PACKAGE" | grep -a -q -w -e "$(hostname)" -e "$HOME"; then
        echo "This computer's name or home is in $PACKAGE" >&2
        exit 1
    fi
done

(cd dist && sha256sum Burin-* > SHA256SUMS)
ls -la dist
