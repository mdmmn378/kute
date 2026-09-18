#!/usr/bin/env bash
# End-to-end test of install.sh against a fake GitHub release, so the whole
# download -> verify -> extract -> install path runs without network access.
#
#   bash scripts/test-install.sh .
set -euo pipefail

PROJECT="${1:-.}"
PROJECT="$(cd "$PROJECT" && pwd)"
[ -f "$PROJECT/Cargo.toml" ] || { echo "not a kute checkout: $PROJECT" >&2; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

FIXTURES="$WORK/fixtures"
FAKEBIN="$WORK/bin"
mkdir -p "$FIXTURES" "$FAKEBIN"

VERSION="1.0.0"
TARGET="x86_64-unknown-linux-gnu"
NAME="kute-${VERSION}-${TARGET}"

# ── build the fake release assets from a real binary ────────────────────────
if [ ! -x "$PROJECT/target/release/kute" ]; then
  (cd "$PROJECT" && cargo build --release --quiet)
fi

STAGE="$WORK/$NAME"
mkdir -p "$STAGE"
cp "$PROJECT/target/release/kute" "$STAGE/kute"
cp "$PROJECT/README.md" "$STAGE/"
tar -C "$WORK" -czf "$FIXTURES/${NAME}.tar.gz" "$NAME"
(cd "$FIXTURES" && sha256sum "${NAME}.tar.gz" > SHA256SUMS.txt)
rm -rf "$STAGE"

# ── a curl that serves the fixtures, and emulates the /releases/latest redirect ─
cat > "$FAKEBIN/curl" <<'FAKE'
#!/bin/sh
out=""; url=""; fmt=""
while [ $# -gt 0 ]; do
  case "$1" in
    -o) out="$2"; shift 2 ;;
    -w) fmt="$2"; shift 2 ;;
    -*) shift ;;
    *) url="$1"; shift ;;
  esac
done

# `curl -I -w '%{url_effective}'` is how install.sh resolves "latest".
if [ -n "$fmt" ]; then
  printf '%s\n' "https://github.com/test/kute/releases/tag/v1.0.0"
  exit 0
fi

file="$FAKE_FIXTURES/${url##*/}"
if [ ! -f "$file" ]; then
  echo "curl: (22) The requested URL returned error: 404" >&2
  exit 22
fi

if [ -n "$out" ] && [ "$out" != "/dev/null" ]; then
  cp "$file" "$out"
else
  cat "$file"
fi
FAKE
chmod +x "$FAKEBIN/curl"

export FAKE_FIXTURES="$FIXTURES"
export PATH="$FAKEBIN:$PATH"

pass() { printf '  ok   %s\n' "$1"; }
fail() { printf '  FAIL %s\n' "$1"; exit 1; }

echo "testing install.sh against a stubbed release"

# ── happy path ──────────────────────────────────────────────────────────────
INSTALL_DIR="$WORK/install"
out="$(KUTE_REPO=test/kute KUTE_INSTALL_DIR="$INSTALL_DIR" sh "$PROJECT/install.sh" 2>&1)" \
  || fail "installer exited non-zero: $out"
echo "$out" | grep -q "Installing kute 1.0.0 (${TARGET})" || fail "wrong target reported: $out"
echo "$out" | grep -q "checksum verified" || fail "checksum was not verified"
[ -x "$INSTALL_DIR/kute" ] || fail "binary was not installed"
"$INSTALL_DIR/kute" --version | grep -q "^kute " || fail "installed binary does not run"
pass "installs the latest release and verifies the checksum"

# ── pinned version ──────────────────────────────────────────────────────────
out="$(KUTE_REPO=test/kute KUTE_VERSION=v1.0.0 KUTE_INSTALL_DIR="$WORK/i2" sh "$PROJECT/install.sh" 2>&1)" \
  || fail "pinned install failed: $out"
echo "$out" | grep -q "Installing kute 1.0.0" || fail "pinned version ignored: $out"
pass "accepts an explicit KUTE_VERSION"

# ── opting out of verification ──────────────────────────────────────────────
out="$(KUTE_REPO=test/kute KUTE_NO_VERIFY=1 KUTE_INSTALL_DIR="$WORK/i3" sh "$PROJECT/install.sh" 2>&1)" \
  || fail "install with verification off failed: $out"
echo "$out" | grep -q "skipped" || fail "expected a skip notice: $out"
pass "honours KUTE_NO_VERIFY"

# ── a tampered archive must not be installed ────────────────────────────────
cp "$FIXTURES/${NAME}.tar.gz" "$FIXTURES/${NAME}.tar.gz.bak"
printf 'tampered' >> "$FIXTURES/${NAME}.tar.gz"
if KUTE_REPO=test/kute KUTE_INSTALL_DIR="$WORK/i4" sh "$PROJECT/install.sh" >/dev/null 2>&1; then
  fail "a tampered archive was installed"
fi
mv "$FIXTURES/${NAME}.tar.gz.bak" "$FIXTURES/${NAME}.tar.gz"
pass "refuses a checksum mismatch"

# ── a missing release explains itself ───────────────────────────────────────
rm -f "$FIXTURES/${NAME}.tar.gz"
if out="$(KUTE_REPO=test/kute KUTE_VERSION=9.9.9 KUTE_INSTALL_DIR="$WORK/i5" sh "$PROJECT/install.sh" 2>&1)"; then
  fail "expected failure for a missing release"
fi
echo "$out" | grep -qi "download failed" || fail "unhelpful error: $out"
pass "explains a missing release"

echo
echo "all installer tests passed"
