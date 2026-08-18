#!/usr/bin/env bash
# Print the CHANGELOG.md body for version X.Y.Z (no heading), for gh release --notes.
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "Usage: changelog-notes.sh X.Y.Z" >&2
  exit 2
fi

VERSION="$1"
ROOT="$(git rev-parse --show-toplevel)"
CHANGELOG="${ROOT}/CHANGELOG.md"

python3 - "$CHANGELOG" "$VERSION" <<'PY'
import re, sys
from pathlib import Path

path, version = sys.argv[1], sys.argv[2]
text = Path(path).read_text()
pat = re.compile(
    rf"(?m)^## {re.escape(version)}(?:\s|—|-|$).*?\n(.*?)(?=^## |\Z)",
    re.S,
)
m = pat.search(text)
if not m:
    raise SystemExit(f"no CHANGELOG section for {version}")
body = m.group(1).strip()
if not body:
    raise SystemExit(f"CHANGELOG section for {version} is empty")
print(body)
PY
