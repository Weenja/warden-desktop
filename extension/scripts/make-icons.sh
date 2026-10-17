#!/bin/sh
# Toolbar and store icons from the brand mark (mockups/shared/brand/icon-192.png).
# Grey = "not linked / not signed in" (same look as the mockup's .badge-icon.is-off).
# Needs ImageMagick. The PNGs are committed; run this only when the brand mark changes.
set -e
cd "$(dirname "$0")/.."
src=../mockups/shared/brand/icon-192.png
for s in 16 32 48 128; do
  magick "$src" -resize ${s}x${s} static/icons/icon-$s.png
done
for s in 16 32 48; do
  magick "$src" -resize ${s}x${s} -colorspace Gray -channel A -evaluate multiply 0.55 +channel static/icons/grey-$s.png
done
