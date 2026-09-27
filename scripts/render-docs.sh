#!/usr/bin/env bash
# Renders the README clips (animated WebP) from the real scene, no window needed.
#   scripts/render-docs.sh            # needs ImageMagick 7 (`magick`) with WebP
set -euo pipefail
cd "$(dirname "$0")/.."
bin=target/release/snowlearner
[ -x "$bin" ] || cargo build --release --quiet
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
export SNOWLEARNER_HOME="$tmp/home"   # never touch the user's config/history

clip() { # name scenario seconds-before frames level
  "$bin" --level "$5" snapshot "$tmp/$1.png" --scenario "$2" --seconds "$3" --frames "$4" --fps 15 --scale 2 --seed 11 >/dev/null
  # Pixel art: no smoothing, lossless, loop forever. 15 fps = 7/100 s per frame.
  magick -delay 7 -loop 0 "$tmp/$1"-*.png -define webp:lossless=true -define webp:method=6 "docs/img/$1.webp"
  echo "docs/img/$1.webp ($(du -h "docs/img/$1.webp" | cut -f1))"
}

clip lesson lesson 120 75 committed
clip fight fight 30 60 steady
clip blackhole blackhole 150 80 relentless
