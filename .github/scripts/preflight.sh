#!/usr/bin/env bash
# Verify alnav + alnav-core versions, CHANGELOG, and (optionally) that tag vX.Y.Z
# does not already exist. Run from anywhere; resolves repo root via git.
set -euo pipefail

CHECK_TAG=0
VERSION_OVERRIDE=""

usage() {
  cat <<'EOF'
Usage: preflight.sh [--tag] [X.Y.Z]

  --tag     also fail if local or github remote already has vX.Y.Z
  X.Y.Z     expected version (default: alnav-core [package] version)
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --tag) CHECK_TAG=1; shift ;;
    -h|--help) usage; exit 0 ;;
    -*)
      echo "unknown flag: $1" >&2
      usage >&2
      exit 2
      ;;
    *)
      VERSION_OVERRIDE="$1"
      shift
      ;;
  esac
done

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

pkg_version() {
  python3 - "$1" <<'PY'
import re, sys
from pathlib import Path
text = Path(sys.argv[1]).read_text()
m = re.search(r'(?m)^version\s*=\s*"([^"]+)"', text)
if not m:
    raise SystemExit(f"no package version in {sys.argv[1]}")
print(m.group(1))
PY
}

core_dep_version() {
  python3 - "$1" <<'PY'
import re, sys
from pathlib import Path
text = Path(sys.argv[1]).read_text()
m = re.search(r'alnav-core\s*=\s*\{[^}]*version\s*=\s*"([^"]+)"', text)
if not m:
    raise SystemExit(f"no alnav-core version in {sys.argv[1]}")
print(m.group(1))
PY
}

CORE_TOML="alnav-core/Cargo.toml"
BIN_TOML="alnav/Cargo.toml"
CHANGELOG="CHANGELOG.md"

CORE_VER="$(pkg_version "$CORE_TOML")"
BIN_VER="$(pkg_version "$BIN_TOML")"
DEP_VER="$(core_dep_version "$BIN_TOML")"
VERSION="${VERSION_OVERRIDE:-$CORE_VER}"

fail=0
check() {
  local ok="$1" msg="$2"
  if [[ "$ok" == "1" ]]; then
    echo "ok  $msg"
  else
    echo "FAIL  $msg" >&2
    fail=1
  fi
}

[[ "$CORE_VER" == "$BIN_VER" ]] && eq=1 || eq=0
check "$eq" "package versions match ($CORE_VER vs $BIN_VER)"

[[ "$DEP_VER" == "$CORE_VER" ]] && eq=1 || eq=0
check "$eq" "alnav depends on alnav-core $DEP_VER (want $CORE_VER)"

[[ "$CORE_VER" == "$VERSION" ]] && eq=1 || eq=0
check "$eq" "Cargo.toml version is $VERSION (got $CORE_VER)"

if grep -Eq "^## ${VERSION}([[:space:]]|—|-|$)" "$CHANGELOG"; then
  check 1 "CHANGELOG.md has ## $VERSION"
else
  check 0 "CHANGELOG.md missing ## $VERSION section"
fi

if [[ "$CHECK_TAG" == "1" ]]; then
  TAG="v${VERSION}"
  if git rev-parse -q --verify "refs/tags/${TAG}" >/dev/null; then
    check 0 "local tag ${TAG} must not exist yet"
  else
    check 1 "no local tag ${TAG}"
  fi
  if git remote get-url github >/dev/null 2>&1; then
    if git ls-remote --exit-code --tags github "refs/tags/${TAG}" >/dev/null 2>&1; then
      check 0 "github remote tag ${TAG} must not exist yet"
    else
      check 1 "no github remote tag ${TAG}"
    fi
  else
    echo "warn  no remote named github; skipped remote tag check" >&2
  fi
fi

if [[ "$fail" != "0" ]]; then
  exit 1
fi
echo "preflight passed for ${VERSION}"
