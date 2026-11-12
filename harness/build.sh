#!/bin/bash
# Compiles everything under src/ against the vanilla 26.2 server jar and its bundled libraries.
# MCB_OUT=<dir> compiles into another directory (default build/classes).
set -e
cd /work
CP=$(./mc-classpath.sh)
OUT=${MCB_OUT:-build/classes}
mkdir -p "$OUT"
find src -name '*.java' > build/sources.txt
set +e
javac -nowarn -Xlint:-options -proc:none -cp "$CP" -d "$OUT" @build/sources.txt > build/javac.log 2>&1
rc=$?
grep -v '^Note:' build/javac.log
if [ $rc -ne 0 ]; then echo "BUILD FAILED"; exit 1; fi
echo "build ok: $(find "$OUT" -name '*.class' | wc -l) classes in $OUT"
