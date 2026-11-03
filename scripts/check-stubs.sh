#!/usr/bin/env bash
# Fails if any placeholder or unfinished-code marker exists in the sources.
set -euo pipefail
cd "$(dirname "$0")/.."
pattern='todo!|unimplemented!|TODO|FIXME|XXX|not implemented|left as an exercise'
if grep -rnE --include='*.rs' --include='*.md' --include='*.toml' --include='*.yml' \
    --exclude-dir=target --exclude-dir=.git --exclude=check-stubs.sh "$pattern" . ; then
  echo "check-stubs: placeholder markers found" >&2
  exit 1
fi
echo "check-stubs: clean"
