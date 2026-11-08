#!/usr/bin/env bash
# Makes dist-site/: the website (the page, its pictures, and the web version in app/), to put on
# GitHub Pages or any web host as it is. Run release/web.sh first, so dist-web/ holds the web version.
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")/.."

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
[ -f dist-web/burin_bg.wasm ] || { echo "dist-web/ is empty: run release/web.sh first" >&2; exit 1; }

OUT=dist-site
rm -rf "$OUT"
mkdir -p "$OUT/docs" "$OUT/app"
sed "s/{{VERSION}}/$VERSION/g" site/index.html > "$OUT/index.html"
cp site/style.css "$OUT/"
cp release/burin.png "$OUT/"
for picture in apercu-3d weave sweep adaptive nesting poignees pieces fiche-de-travail preferences; do
    cp "docs/$picture.png" "$OUT/docs/"
done
cp dist-web/* "$OUT/app/"
# GitHub Pages serves the files as they are, at the project's own address.
touch "$OUT/.nojekyll"
echo "burin.cc" > "$OUT/CNAME"
echo "dist-site/ is ready for Burin $VERSION"
