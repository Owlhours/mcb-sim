#!/usr/bin/env bash
# Fails if game files, compiled code or other things that must not be published are tracked in git.
# Usage: tools/check-public-tree.sh [--history]   (--history checks every commit, not just the current tree)
set -euo pipefail
cd "$(dirname "$0")/.."
bad=0
if [ "${1:-}" = "--history" ]; then
    list() { git rev-list --objects --all | awk 'NF==2 {print $2}' | sort -u; }
else
    list() { git ls-files; }
fi
while IFS= read -r f; do
    case "$f" in
        *.jar|*.class|*.zip|*.mca|*.mcr|*.dat|*.litematic|*.schem|*.schematic|*.nbt|*.ogg|*.sf2)
            echo "not allowed (game file, schematic or binary): $f"; bad=1 ;;
        */net/minecraft/*|net/minecraft/*|*/com/mojang/*|com/mojang/*)
            echo "not allowed (Minecraft package path): $f"; bad=1 ;;
    esac
done < <(list)
# large files: anything over 2 MB in the current tree
while IFS= read -r f; do
    [ -f "$f" ] || continue
    s=$(stat -c %s "$f")
    if [ "$s" -gt 2097152 ]; then echo "too large ($s bytes): $f"; bad=1; fi
done < <(git ls-files)
if [ $bad -ne 0 ]; then echo "public-tree check FAILED"; exit 1; fi
echo "public-tree check passed"
