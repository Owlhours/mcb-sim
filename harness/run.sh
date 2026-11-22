#!/usr/bin/env bash
# The headless Java harness: the bot in Mojang's own 26.2 server code, in-process, no network, no window.
#
#   harness/run.sh build <schematic> [quiet] [wall-seconds]   one build, start to verdict; last line VERDICT ...
#   harness/run.sh prep <schematic>...                        normalise files into work/corpus (input of the Rust sims);
#                                                             their hashes also go to work/corpus/prep-list.txt
#   harness/run.sh tables                                     write work/reference/mc/states.tsv and java-extra/*
#   harness/run.sh one <hash> [quiet] [wall-seconds]          one build from work/corpus by hash (prefix ok)
#   harness/run.sh corpus <list> <out.tsv> [max] [max-wall-s] [build-wall-s]   a list of hashes, one row each
#   harness/run.sh java <main-class> [args...]                any main class on the harness classpath
#
# Builds on first use (harness/build.sh). Data that is not in git lives in $MCB_WORK (default <repo>/work);
# the game's own log goes to $MCB_WORK/logs.
#   JAVA_OPTS     JVM options (default -Xmx4g -XX:+UseSerialGC)
#   MCB_CLASSES   compiled classes (default harness/build/classes)
#   MCB_SNAP=1    run from a private copy of the classes, so rebuilding does not disturb a long batch
#   MCB_SEED=k    the bot starts k blocks further along the front of the build (a different course, same build)
set -euo pipefail
H="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(dirname "$H")"
abs() { case "$1" in /*) echo "$1" ;; *) echo "$PWD/$1" ;; esac; }
cmd=${1:-help}; shift || true
args=()
case "$cmd" in
    build) [ $# -ge 1 ] || { echo "usage: run.sh build <schematic> [quiet] [wall-seconds]" >&2; exit 2; }
           args=(mcb.run.RunFile "$(abs "$1")" "${@:2}") ;;
    prep)  for f in "$@"; do args+=("$(abs "$f")"); done; args=(mcb.run.Prep "${args[@]}") ;;
    tables) args=() ;;
    one)   args=(mcb.run.RunOne "$@") ;;
    corpus) args=(mcb.run.RunCorpus "$(abs "$1")" "$(abs "$2")" "${@:3}") ;;
    java)  args=("$@") ;;
    *) sed -n '2,19p' "$0"; exit 2 ;;
esac
export MCB_WORK="$(abs "${MCB_WORK:-$ROOT/work}")"
CLASSES=$(abs "${MCB_CLASSES:-$H/build/classes}")
if [ -z "${MCB_CLASSES:-}" ] && [ ! -f "$CLASSES/mcb/run/RunFile.class" ]; then "$H/build.sh" >&2; fi
CP=$("$H/mc-classpath.sh")
mkdir -p "$MCB_WORK/tmp"
cd "$MCB_WORK"   # (the game writes its log to ./logs)
JOPTS=${JAVA_OPTS:--Xmx4g -XX:+UseSerialGC}
if [ -n "${MCB_SNAP:-}" ]; then
    SNAP="$MCB_WORK/tmp/classes-$$"
    rm -rf "$SNAP"; cp -r "$CLASSES" "$SNAP"
    trap 'rm -rf "$SNAP"' EXIT
    CLASSES=$SNAP
fi
if [ "$cmd" = tables ]; then
    # shellcheck disable=SC2086
    java $JOPTS -cp "$CLASSES:$CP" mcb.run.DumpStates "$MCB_WORK/reference/mc/states.tsv"
    # shellcheck disable=SC2086
    java $JOPTS -cp "$CLASSES:$CP" mcb.run.DumpRef "$MCB_WORK/reference/java-extra"
    exit 0
fi
# shellcheck disable=SC2086
if [ -n "${MCB_SNAP:-}" ]; then java $JOPTS -cp "$CLASSES:$CP" "${args[@]}"; else exec java $JOPTS -cp "$CLASSES:$CP" "${args[@]}"; fi
