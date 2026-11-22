#!/usr/bin/env bash
# Prints the classpath of the vanilla 26.2 server and its bundled libraries (unpacked by build.sh).
B="$(cd "$(dirname "$0")" && pwd)/.deps/bundle/META-INF"
[ -f "$B/versions/26.2/server-26.2.jar" ] || { echo "no server jar: run harness/build.sh first" >&2; exit 1; }
echo -n "$B/versions/26.2/server-26.2.jar"
find "$B/libraries" -name '*.jar' | sort | while read -r f; do echo -n ":$f"; done
