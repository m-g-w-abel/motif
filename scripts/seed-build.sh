#!/usr/bin/env bash
# Seeds a new worktree's cargo build folder from the main checkout's, so third-party
# dependencies don't compile again. Run it in the worktree before its first build.
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
main=$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")
if [ "$here" = "$main" ]; then
  echo "This is the main checkout; nothing to seed."
  exit 0
fi

mapfile -t src < <(describe "$main")
mapfile -t dst < <(describe "$here")
if [ "${src[0]}" = "${dst[0]}" ]; then
  echo "Both checkouts use ${src[0]}; seeding would share it. Is build-dir set per checkout?" >&2
  exit 1
fi
if [ -e "${dst[0]}/debug" ]; then
  echo "${dst[0]}/debug already exists; not seeding."
  exit 0
fi
if [ ! -d "${src[0]}/debug" ]; then
  echo "The main checkout has no debug build yet; nothing to seed from."
  exit 0
fi

mkdir -p "${dst[0]}"
cp -a "${src[0]}/debug" "${dst[0]}/"
# Incremental data only ever belongs to workspace crates.
rm -rf "${dst[0]}/debug/incremental"
for package in ${dst[1]}; do
  find "${dst[0]}/debug/.fingerprint" "${dst[0]}/debug/build" -maxdepth 1 \
    -regextype posix-extended -regex ".*/${package}-[0-9a-f]{16}" -exec rm -rf {} +
done
echo "Seeded ${dst[0]} from ${src[0]} ($(du -sh "${dst[0]}" | cut -f1))."
