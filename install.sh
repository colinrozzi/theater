#!/bin/sh
# Theater installer — https://github.com/colinrozzi/theater
#
#   curl -fsSL https://colinrozzi.github.io/theater/install.sh | sh
#
# Installs the `theater` CLI to ~/.local/bin. No root, no systemd, no nix.
# It downloads a release binary, VERIFIES its SHA-256 checksum before
# installing, and refuses to install on a mismatch. It is meant to be read
# before you run it — that's the whole point of piping it to your eyes first.
#
# Options (environment variables):
#   VERSION   pin a release tag (e.g. release-20260812-e8affc4). Default: latest.
#   BIN_DIR   install location. Default: ~/.local/bin
#
set -eu

REPO="colinrozzi/theater"
BIN_DIR="${BIN_DIR:-$HOME/.local/bin}"

say()  { printf '%s\n' "$*"; }
err()  { printf 'error: %s\n' "$*" >&2; exit 1; }

# --- [1] platform gate -------------------------------------------------------
# v1 ships linux-x86_64 only. Anything else exits cleanly with a friendly note
# rather than installing something that can't run.
OS="$(uname -s)"
ARCH="$(uname -m)"
case "$OS-$ARCH" in
  Linux-x86_64|Linux-amd64) PLAT="linux-x86_64" ;;
  *)
    say "Theater doesn't ship a $OS/$ARCH build yet — coming soon."
    say "For now: build from source → https://github.com/$REPO"
    exit 0
    ;;
esac

# --- [2] tools we rely on ----------------------------------------------------
need() { command -v "$1" >/dev/null 2>&1 || err "need '$1' on PATH but it's missing"; }
need curl
need tar
# checksum tool: coreutils sha256sum, or BSD/macOS shasum -a 256.
if command -v sha256sum >/dev/null 2>&1; then
  SHACHECK="sha256sum -c"
elif command -v shasum >/dev/null 2>&1; then
  SHACHECK="shasum -a 256 -c"
else
  err "need 'sha256sum' or 'shasum' to verify the download"
fi

# --- [3] resolve the release tag --------------------------------------------
# Default: whatever GitHub calls 'latest'. Override with VERSION=<tag>.
if [ "${VERSION:-}" = "" ]; then
  say "Resolving latest release of $REPO ..."
  # Follow the /releases/latest redirect and read the tag out of the Location.
  TAG="$(curl -fsSLI -o /dev/null -w '%{url_effective}' \
           "https://github.com/$REPO/releases/latest" \
         | sed -n 's#.*/tag/##p')"
  [ -n "$TAG" ] || err "could not resolve the latest release tag"
else
  TAG="$VERSION"
fi
say "Installing theater $TAG ($PLAT)"

# Asset naming: theater-<ver>-<plat>.tar.gz  (ver = tag without the release- prefix)
VER="${TAG#release-}"
ASSET="theater-${VER}-${PLAT}.tar.gz"
BASE="https://github.com/$REPO/releases/download/$TAG"

# --- [4] download into a scratch dir ----------------------------------------
TMP="$(mktemp -d "${TMPDIR:-/tmp}/theater-install.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT INT TERM
say "Downloading $ASSET ..."
curl -fsSL "$BASE/$ASSET"        -o "$TMP/$ASSET"        || err "download failed: $BASE/$ASSET"
curl -fsSL "$BASE/$ASSET.sha256" -o "$TMP/$ASSET.sha256" || err "download failed: $BASE/$ASSET.sha256"

# --- [5] VERIFY THE CHECKSUM (non-negotiable; fail hard on mismatch) --------
# The .sha256 is `sha256sum` output (<hash>  <filename>); verify in-place so the
# filename in it matches what we downloaded.
say "Verifying SHA-256 ..."
( cd "$TMP" && $SHACHECK "$ASSET.sha256" >/dev/null 2>&1 ) \
  || err "CHECKSUM MISMATCH for $ASSET — refusing to install (corrupt or tampered download)"
say "Checksum OK."

# --- [6] extract + install ---------------------------------------------------
tar -xzf "$TMP/$ASSET" -C "$TMP"
SRC="$(find "$TMP" -type f -name theater ! -name '*.tar.gz' | head -n1)"
[ -n "$SRC" ] || err "no 'theater' binary found inside $ASSET"

mkdir -p "$BIN_DIR"
install -m 0755 "$SRC" "$BIN_DIR/theater" 2>/dev/null \
  || { cp "$SRC" "$BIN_DIR/theater" && chmod 0755 "$BIN_DIR/theater"; }
say "Installed: $BIN_DIR/theater"

# --- [7] PATH note -----------------------------------------------------------
case ":$PATH:" in
  *":$BIN_DIR:"*) ONPATH=1 ;;
  *)              ONPATH=0 ;;
esac

say ""
say "Theater is installed. 🎭"
if [ "$ONPATH" -eq 0 ]; then
  say "Add it to your PATH (then restart your shell):"
  say "    echo 'export PATH=\"$BIN_DIR:\$PATH\"' >> ~/.profile"
  say ""
fi
say "Get started:"
if [ "$ONPATH" -eq 0 ]; then
  say "    $BIN_DIR/theater --version      # confirm the install"
  say "    $BIN_DIR/theater create hello   # scaffold your first actor"
else
  say "    theater --version               # confirm the install"
  say "    theater create hello            # scaffold your first actor"
fi
say ""
say "Docs: https://github.com/$REPO"
