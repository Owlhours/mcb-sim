#!/bin/bash
# run.sh <main-class> [args...]   (JAVA_OPTS to override heap etc.)
cd /work
CP=$(./mc-classpath.sh)
exec java ${JAVA_OPTS:--Xmx4g -XX:+UseSerialGC} -Dlog4j2.formatMsgNoLookups=true -cp "build/classes:$CP" "$@"
