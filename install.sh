#!/bin/sh
# Installs snowlearner in one step (Linux x86_64, macOS Apple Silicon):
#   curl -fsSL https://raw.githubusercontent.com/AxeForging/snowlearner/main/install.sh | sh
#   curl -fsSL https://raw.githubusercontent.com/AxeForging/snowlearner/main/install.sh | sh -s -- --lang es
# Downloads the latest release, checks its SHA-256, puts the binary in ~/.local/bin
# (SNOWLEARNER_BIN_DIR to change) and runs `snowlearner setup` with any arguments given.
# SNOWLEARNER_VERSION=v0.1.0 pins a version; SNOWLEARNER_DOWNLOAD_BASE points at a mirror.
set -eu

REPO="AxeForging/snowlearner"
VERSION="${SNOWLEARNER_VERSION:-latest}"
BIN_DIR="${SNOWLEARNER_BIN_DIR:-$HOME/.local/bin}"

fail() {
    echo "snowlearner: $*" >&2
    exit 1
}

case "$(uname -s)-$(uname -m)" in
    Linux-x86_64 | Linux-amd64) target="x86_64-unknown-linux-gnu" ;;
    Darwin-arm64) target="aarch64-apple-darwin" ;;
    *) fail "ainda não há binário pronto para $(uname -s) $(uname -m); compile com: cargo install --git https://github.com/$REPO" ;;
esac

asset="snowlearner-$target.tar.gz"
if [ -n "${SNOWLEARNER_DOWNLOAD_BASE:-}" ]; then
    base="$SNOWLEARNER_DOWNLOAD_BASE" # mirrors and tests
elif [ "$VERSION" = latest ]; then
    base="https://github.com/$REPO/releases/latest/download"
else
    base="https://github.com/$REPO/releases/download/$VERSION"
fi

fetch() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$1" -o "$2"
    elif command -v wget >/dev/null 2>&1; then
        wget -q "$1" -O "$2"
    else
        fail "preciso de curl ou wget"
    fi
}

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM

echo "Baixando snowlearner ($target)..."
fetch "$base/$asset" "$tmp/$asset" || fail "não consegui baixar $base/$asset"
fetch "$base/$asset.sha256" "$tmp/$asset.sha256" || fail "não consegui baixar o checksum"

expected="$(cut -d ' ' -f 1 <"$tmp/$asset.sha256")"
if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$tmp/$asset" | cut -d ' ' -f 1)"
else
    actual="$(shasum -a 256 "$tmp/$asset" | cut -d ' ' -f 1)"
fi
[ "$expected" = "$actual" ] || fail "checksum não confere (esperado $expected, veio $actual)"

tar -xzf "$tmp/$asset" -C "$tmp"
mkdir -p "$BIN_DIR"
cp "$tmp/snowlearner-$target/snowlearner" "$BIN_DIR/snowlearner.new"
chmod 755 "$BIN_DIR/snowlearner.new"
mv -f "$BIN_DIR/snowlearner.new" "$BIN_DIR/snowlearner"
if [ "$(uname -s)" = Darwin ]; then
    xattr -d com.apple.quarantine "$BIN_DIR/snowlearner" 2>/dev/null || true
fi
echo "Instalado em $BIN_DIR/snowlearner"
echo

"$BIN_DIR/snowlearner" setup "$@"

# shellcheck disable=SC2016 # the printed $PATH is meant literally: it's a line to paste.
case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) printf '\nAdicione %s ao PATH para usar o comando `snowlearner`:\n  echo '\''export PATH="%s:$PATH"'\'' >> ~/.profile\n' "$BIN_DIR" "$BIN_DIR" ;;
esac
