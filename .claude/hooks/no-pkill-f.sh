#!/bin/sh
# PreToolUse hook for Bash: refuse `pkill -f` / `pgrep -f`. The pattern also
# appears in the shell's own command line, so it matches (and kills, or never
# stops waiting on) the very shell running it.
cmd=$(jq -r '.tool_input.command // empty')
if printf '%s' "$cmd" | grep -Eq '(^|[;&|([:space:]])(pkill|pgrep)[[:space:]]([^;&|]*[[:space:]])?(-[a-zA-Z]*f|--full)'; then
  echo 'pkill/pgrep -f matches its own shell. Find the PID with `ss -ltnp` or /proc/<pid>/cmdline and kill it by PID; to wait on a build, run it in the background and wait for its notification.' >&2
  exit 2
fi
