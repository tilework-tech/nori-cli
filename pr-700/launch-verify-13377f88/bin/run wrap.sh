#!/bin/sh
# Synthetic wrapper: record our own argv, then run everything after our delimiter.
python3 -c 'import json,os,sys; json.dump({"pid":os.getppid(),"argv":sys.argv[1:]},open(os.environ["PROBE_OUT"]+"/wrapper-%d.json"%os.getppid(),"w"))' "$@"
[ "$1" = "--tag" ] && [ "$2" = "w 1" ] && [ "$3" = "--" ] || exit 67
shift 3
exec "$@"
