#!/usr/bin/env bash
# Compiles the bot and the headless runners against the vanilla 26.2 server classes. The bot itself (its planner
# and its Minecraft adapter) is not copied here: it is compiled from an MCB Builder checkout (see mcb-builder.sh).
#
# Minecraft 26.2 ships unobfuscated, so this compiles straight against the server jar: no Gradle, no remapping.
# On the first run it downloads the vanilla 26.2 server jar from Mojang into harness/.deps/ (never committed),
# checks it against a pinned SHA-1 and unpacks its inner server jar and libraries.
#
# Needs: bash, curl, sha1sum, and a JDK 25 (javac; its `jar` unpacks the bundle when unzip is missing).
# MCB_OUT=<dir> compiles into another directory (default harness/build/classes).
# MCB_BUILDER=<dir> uses that MCB Builder checkout (default: ../mcb-builder next to this repository, else a clone of
# the commit pinned in mcb-builder.ref, made in harness/.deps/mcb-builder).
set -euo pipefail
cd "$(dirname "$0")"
DEPS=.deps

fetch() {  # url file sha1
    if [ ! -f "$2" ] || ! echo "$3  $2" | sha1sum -c --status; then
        echo "downloading $(basename "$2")"
        command -v curl > /dev/null || { echo "curl is needed to download $1 (or put the file at $2)" >&2; exit 1; }
        curl -fsSL -o "$2.part" "$1"
        echo "$3  $2.part" | sha1sum -c --status || { echo "SHA-1 mismatch for $1" >&2; exit 1; }
        mv "$2.part" "$2"
    fi
}

mkdir -p "$DEPS"
fetch https://piston-data.mojang.com/v1/objects/823e2250d24b3ddac457a60c92a6a941943fcd6a/server.jar \
      "$DEPS/server-26.2-bundler.jar" 823e2250d24b3ddac457a60c92a6a941943fcd6a

B="$DEPS/bundle/META-INF"
if [ ! -f "$B/versions/26.2/server-26.2.jar" ]; then
    rm -rf "$DEPS/bundle"
    if command -v unzip > /dev/null; then
        unzip -q "$DEPS/server-26.2-bundler.jar" 'META-INF/versions/*' 'META-INF/libraries/*' -d "$DEPS/bundle"
    else
        mkdir -p "$DEPS/bundle" && (cd "$DEPS/bundle" && jar xf ../server-26.2-bundler.jar META-INF/versions META-INF/libraries)
    fi
fi

CP=$(./mc-classpath.sh)
OUT=${MCB_OUT:-build/classes}
mkdir -p build "$OUT"
MB=$(./mcb-builder.sh)
{ find "$MB/core/src" "$MB/adapter/src" "$MB/adapter/server/src" "$MB/adapter/headless/src" "$MB/gate/src" -name '*.java'; find src -name '*.java'; } | sort > build/sources.txt
set +e
javac -nowarn -Xlint:-options -proc:none -encoding UTF-8 -cp "$CP" -d "$OUT" @build/sources.txt > build/javac.log 2>&1
rc=$?
set -e
grep -v '^Note:' build/javac.log || true
if [ $rc -ne 0 ]; then echo "BUILD FAILED"; exit 1; fi
echo "build ok: $(find "$OUT" -name '*.class' | wc -l) classes in harness/$OUT (MCB Builder from $MB)"
