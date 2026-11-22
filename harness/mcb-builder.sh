#!/usr/bin/env bash
# Prints the directory of the MCB Builder checkout the harness compiles the bot from:
#   1. $MCB_BUILDER, if set;
#   2. ../mcb-builder next to this repository (the usual layout when working on both);
#   3. otherwise the source of https://github.com/Owlhours/mcb-builder at the commit in mcb-builder.ref, downloaded
#      once into harness/.deps/mcb-builder-<commit>.
# A sibling checkout at another commit than the pinned one is used as it is, with a note on stderr.
set -euo pipefail
H="$(cd "$(dirname "$0")" && pwd)"
REF=$(grep -v '^#' "$H/mcb-builder.ref" | head -1 | awk '{print $1}')
if [ -n "${MCB_BUILDER:-}" ]; then cd "$MCB_BUILDER" && pwd; exit 0; fi
SIB="$(dirname "$H")/../mcb-builder"
if [ -d "$SIB/core/src/mcb" ]; then
    SIB="$(cd "$SIB" && pwd)"
    have=$(git -C "$SIB" rev-parse HEAD 2>/dev/null || echo unknown)
    [ "$have" = "$REF" ] || echo "note: using $SIB at $have (mcb-builder.ref pins $REF)" >&2
    echo "$SIB"; exit 0
fi
D="$H/.deps/mcb-builder-$REF"
if [ ! -d "$D/core/src/mcb" ]; then
    # (a source archive of that commit; no git needed)
    echo "downloading MCB Builder $REF" >&2
    rm -rf "$D.part"; mkdir -p "$D.part"
    curl -fsSL "https://codeload.github.com/Owlhours/mcb-builder/tar.gz/$REF" | tar -xz -C "$D.part" --strip-components=1
    mv "$D.part" "$D"
fi
echo "$D"
