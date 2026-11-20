#!/usr/bin/env bash
# Renders the README clips (animated WebP) from the real scene, no window needed.
#   scripts/render-docs.sh            # every clip
#   scripts/render-docs.sh comet      # only the clips named
# Needs ImageMagick 7 (`magick`) with WebP, or ffmpeg with libwebp. SNOWLEARNER_BIN
# picks the binary (a release one, on a machine without Rust); else it is built.
set -euo pipefail
cd "$(dirname "$0")/.."
bin=${SNOWLEARNER_BIN:-target/release/snowlearner}
[ -x "$bin" ] || cargo build --release --quiet
if command -v magick >/dev/null; then
  tool=magick
elif command -v ffmpeg >/dev/null && [[ $(ffmpeg -hide_banner -encoders 2>/dev/null) == *libwebp_anim* ]]; then
  tool=ffmpeg
else
  echo "render-docs needs ImageMagick 7 (magick) or ffmpeg with libwebp" >&2
  exit 1
fi
only=("$@")
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
export SNOWLEARNER_HOME="$tmp/home"   # never touch the user's config/history

wanted() { # name: on the command line, or nothing named
  [ ${#only[@]} -eq 0 ] && return 0
  local n
  for n in "${only[@]}"; do [ "$n" = "$1" ] && return 0; done
  return 1
}

# Frames "$1"-*.png → docs/img/$2.webp. Pixel art: no smoothing, lossless, loop
# forever. 15 fps = 7/100 s per frame.
animate() { # frames-prefix name
  if [ "$tool" = magick ]; then
    magick -delay 7 -loop 0 "$1"-*.png -define webp:lossless=true -define webp:method=6 "docs/img/$2.webp"
  else
    ffmpeg -loglevel error -y -framerate 100/7 -pattern_type glob -i "$1-*.png" \
      -c:v libwebp_anim -lossless 1 -compression_level 6 -loop 0 "docs/img/$2.webp"
  fi
  echo "docs/img/$2.webp ($(du -h "docs/img/$2.webp" | cut -f1))"
}

clip() { # name scenario seconds-before frames level
  wanted "$1" || return 0
  "$bin" --level "$5" snapshot "$tmp/$1.png" --scenario "$2" --seconds "$3" --frames "$4" --fps 15 --scale 2 --seed 11 >/dev/null
  animate "$tmp/$1" "$1"
}

# The overlay as people really use it: transparent frames composited over a
# neutral work screen (a local fake editor page, rendered with a throwaway
# Chromium profile — nothing from this machine ends up in the image).
overlay_clip() { # name scenario seconds-before frames level
  wanted "$1" || return 0
  local chrome
  chrome=$(command -v chromium-browser || command -v chromium || command -v google-chrome) || { echo "overlay clip needs Chromium" >&2; return 1; }
  "$chrome" --headless=new --disable-gpu --no-first-run --user-data-dir="$tmp/chrome" --hide-scrollbars \
    --window-size=800,450 --screenshot="$tmp/backdrop.png" "file://$PWD/scripts/docs-backdrop.html" 2>/dev/null
  "$bin" --level "$5" snapshot "$tmp/$1.png" --overlay --scenario "$2" --seconds "$3" --frames "$4" --fps 15 \
    --width 400 --height 225 --scale 2 --seed 11 >/dev/null
  for f in "$tmp/$1"-*.png; do
    if [ "$tool" = magick ]; then
      magick "$tmp/backdrop.png" "$f" -composite "$f"
    else
      ffmpeg -loglevel error -y -i "$tmp/backdrop.png" -i "$f" -filter_complex overlay "$f.tmp.png" && mv "$f.tmp.png" "$f"
    fi
  done
  animate "$tmp/$1" "$1"
}

overlay_clip overlay lesson 240 75 committed
clip lesson lesson 120 75 committed
clip fight fight 30 60 steady
clip snowstorm snowstorm 30 90 relentless
clip comet comet 30 75 steady
clip blackhole blackhole 150 80 relentless
