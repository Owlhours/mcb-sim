#!/bin/bash
# prints the classpath of the vanilla server + bundled libraries
B=/work/vendor/mc/bundle/META-INF
echo -n "$B/versions/26.2/server-26.2.jar"
find $B/libraries -name '*.jar' | sort | while read f; do echo -n ":$f"; done
