#!/bin/bash
# run.sh <main-class> [args...]
#   JAVA_OPTS     override JVM options
#   MCB_CLASSES   directory of compiled classes (default build/classes)
#   MCB_SNAP=1    run from a private copy of the classes (long batches: rebuilding then does not disturb them)
cd /work
CP=$(./mc-classpath.sh)
CLASSES=${MCB_CLASSES:-build/classes}
if [ -n "$MCB_SNAP" ]; then
  SNAP=build/snap-$$
  rm -rf "$SNAP"; cp -r "$CLASSES" "$SNAP"
  trap 'rm -rf "/work/$SNAP"' EXIT
  java ${JAVA_OPTS:--Xmx4g -XX:+UseSerialGC} -cp "$SNAP:$CP" "$@"
  exit $?
fi
exec java ${JAVA_OPTS:--Xmx4g -XX:+UseSerialGC} -cp "$CLASSES:$CP" "$@"
