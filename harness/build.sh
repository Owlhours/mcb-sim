#!/bin/bash
# Compiles everything under src/ against the vanilla 26.2 server jar and its bundled libraries.
set -e
cd /work
CP=$(./mc-classpath.sh)
mkdir -p build/classes
find src -name '*.java' > build/sources.txt
javac -nowarn -Xlint:-options -proc:none -cp "$CP" -d build/classes @build/sources.txt 2>&1 | grep -v '^Note:' || true
echo "build ok: $(find build/classes -name '*.class' | wc -l) classes"
