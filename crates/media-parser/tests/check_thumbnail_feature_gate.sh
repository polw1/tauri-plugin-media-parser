#!/bin/sh
set -eu

workspace_root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
output_file=$(mktemp)
trap 'rm -f "$output_file"' EXIT

if cargo check \
   --manifest-path "$workspace_root/Cargo.toml" \
   -p media-parser \
   --no-default-features \
   --features thumbnails >"$output_file" 2>&1
then
   echo "expected the thumbnails-only build to fail" >&2
   exit 1
fi

if ! grep -Fq 'feature `thumbnails` requires exactly one H.264 decoder backend' "$output_file"
then
   cat "$output_file" >&2
   echo "the build failed without the expected backend feature message" >&2
   exit 1
fi
