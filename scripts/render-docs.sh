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

# The overlay as people really use it: transparent frames composited over a
# neutral work screen (a local fake editor page, rendered with a throwaway
# Chromium profile — nothing from this machine ends up in the image).
overlay_clip() { # name scenario seconds-before frames level
  local chrome
  chrome=$(command -v chromium-browser || command -v chromium || command -v google-chrome) || { echo "overlay clip needs Chromium" >&2; return 1; }
  "$chrome" --headless=new --disable-gpu --no-first-run --user-data-dir="$tmp/chrome" --hide-scrollbars \
    --window-size=800,450 --screenshot="$tmp/backdrop.png" "file://$PWD/scripts/docs-backdrop.html" 2>/dev/null
  "$bin" --level "$5" snapshot "$tmp/$1.png" --overlay --scenario "$2" --seconds "$3" --frames "$4" --fps 15 \
    --width 400 --height 225 --scale 2 --seed 11 >/dev/null
  for f in "$tmp/$1"-*.png; do magick "$tmp/backdrop.png" "$f" -composite "$f"; done
  magick -delay 7 -loop 0 "$tmp/$1"-*.png -define webp:lossless=true -define webp:method=6 "docs/img/$1.webp"
  echo "docs/img/$1.webp ($(du -h "docs/img/$1.webp" | cut -f1))"
}

overlay_clip overlay lesson 240 75 committed
clip lesson lesson 120 75 committed
clip fight fight 30 60 steady
clip snowstorm snowstorm 30 90 relentless
clip blackhole blackhole 150 80 relentless
