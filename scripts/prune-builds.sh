#!/usr/bin/env bash
# Cargo never deletes old build outputs, so each checkout's build folder grows with
# every new mix of flags and features (22 GB for the main checkout by 2026-11-25),
# and a removed worktree leaves its folder behind. This deletes the folders of
# checkouts that no longer exist and cleans our own crates in the rest (cargo clean
# waits for a running build; third-party builds stay, so rebuilds take minutes).
# Never delete single files by age instead: a build script's output is only read
# when its crate recompiles, and cargo doesn't notice it's gone.
# Run weekly by ~/.config/systemd/user/motif-prune-builds.timer; safe to run any time.
set -euo pipefail

root=/var/tmp/motif-build
main=$(dirname "$(git -C "$(dirname "$0")" rev-parse --path-format=absolute --git-common-dir)")

# One at a time with seeding, so a worktree being seeded isn't pruned halfway.
if [ -z "${PRUNE_LOCKED:-}" ]; then
  mkdir -p "$root"
  PRUNE_LOCKED=1 exec flock "$root/.seed.lock" nice -n 19 ionice -c 3 "$0" "$@"
fi

keep=()
while read -r checkout; do
  [ -f "$checkout/Cargo.toml" ] || continue
  mapfile -t info < <(cd "$checkout" && cargo metadata --format-version 1 --no-deps | python3 -c '
import json, sys
m = json.load(sys.stdin)
print(m.get("build_directory") or m["target_directory"])
print(" ".join("-p " + p["name"] for p in m["packages"]))')
  dir=${info[0]}
  read -ra packages <<< "${info[1]}"
  keep+=("$dir")
  [ -d "$dir" ] || continue
  cargo clean --manifest-path "$checkout/Cargo.toml" "${packages[@]}" 2>&1 | tail -n 1
  # Each cross build is a folder named after its target (Android's aarch64 one).
  for t in "$dir"/*/debug; do
    [ -d "$t" ] || continue
    cargo clean --manifest-path "$checkout/Cargo.toml" --target "$(basename "$(dirname "$t")")" \
      "${packages[@]}" 2>&1 | tail -n 1
  done
done < <(git -C "$main" worktree list --porcelain | sed -n 's/^worktree //p')

# Folders are $root/<2 hex>/<hash>.
for dir in "$root"/*/*/; do
  dir=${dir%/}
  printf '%s\n' "${keep[@]}" | grep -qxF "$dir" && continue
  echo "Deleting $dir (its checkout is gone)"
  rm -rf "$dir"
done
find "$root" -mindepth 1 -maxdepth 1 -type d -empty -delete
du -sh "$root"
