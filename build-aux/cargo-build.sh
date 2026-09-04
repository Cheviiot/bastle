#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
set -eu

cargo_bin=$1
source_dir=$2
target_dir=$3
profile=$4
offline=$5
features=$6
output=$7

set -- build --manifest-path "$source_dir/Cargo.toml" --target-dir "$target_dir"
if [ "$profile" = release ]; then
  set -- "$@" --release
fi
if [ "$offline" = true ]; then
  set -- "$@" --offline
fi
if [ -n "$features" ]; then
  set -- "$@" --features "$features"
fi
"$cargo_bin" "$@"
cp "$target_dir/$profile/bastle" "$output"
