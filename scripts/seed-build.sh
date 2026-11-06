#!/usr/bin/env bash
# Seeds a new worktree's cargo build folder from the main checkout's (or from the
# checkout given as the argument), so third-party dependencies don't compile again:
# the host build and every cross build in it (Android's aarch64 one). Run it in the
# worktree before its first build; run it again to add what the source built since.
#
# The workspace crates' build records are deleted so they always rebuild from this
# checkout's code: cargo hashes workspace crates without the checkout path, so a
# copied record could otherwise pass another checkout's build off as this one's.
set -euo pipefail

# Prints a checkout's build folder, then its workspace package names.
describe() {
  (cd "$1" && cargo metadata --format-version 1 --no-deps) | python3 -c '
import json, sys
m = json.load(sys.stdin)
print(m.get("build_directory") or m["target_directory"])
print(" ".join(p["name"] for p in m["packages"]))'
}

here=$(git rev-parse --show-toplevel)
main=${1:-$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")}
if [ "$here" = "$main" ]; then
  echo "This is the checkout to seed from; nothing to seed."
  exit 0
fi

mapfile -t src < <(describe "$main")
mapfile -t dst < <(describe "$here")
if [ "${src[0]}" = "${dst[0]}" ]; then
  echo "Both checkouts use ${src[0]}; seeding would share it. Is build-dir set per checkout?" >&2
  exit 1
fi
# Merged, never overwritten: a cross build also adds host tools (build scripts,
# macros) to debug/, so a checkout that has a host build can still lack those.
# Incremental data only ever belongs to workspace crates.
# CMake caches record the absolute build folder and refuse to run anywhere else.
excludes=(--exclude=/incremental --exclude=CMakeCache.txt)
hash=$(printf '[0-9a-f]%.0s' {1..16})
for package in ${dst[1]}; do
  excludes+=("--exclude=/.fingerprint/${package}-$hash" "--exclude=/build/${package}-$hash")
done
mkdir -p "${dst[0]}"
for profile in "${src[0]}"/debug "${src[0]}"/*/debug; do
  [ -d "$profile" ] || continue
  rel=${profile#"${src[0]}"/}
  mkdir -p "${dst[0]}/$rel"
  rsync -a --ignore-existing "${excludes[@]}" "$profile/" "${dst[0]}/$rel/"
  echo "Seeded ${dst[0]}/$rel."
done
