#!/bin/sh
# Installa piadazip — https://github.com/Tnnienn/piadazip
# Uso: curl -fsSL https://raw.githubusercontent.com/Tnnienn/piadazip/main/install.sh | sh
set -e

REPO="Tnnienn/piadazip"
BIN="piadazip"

# ── Rilevamento piattaforma ───────────────────────────────────────────────────

OS=$(uname -s)
ARCH=$(uname -m)

case "$OS" in
  Linux)
    case "$ARCH" in
      x86_64)          TARGET="linux-x86_64" ;;
      aarch64|arm64)   TARGET="linux-aarch64" ;;
      *)
        echo "Architettura non supportata: $ARCH"
        echo "Compila manualmente: cargo build --release"
        exit 1
        ;;
    esac
    ;;
  Darwin)
    case "$ARCH" in
      x86_64)  TARGET="macos-x86_64" ;;
      arm64)   TARGET="macos-aarch64" ;;
      *)
        echo "Architettura non supportata: $ARCH"
        exit 1
        ;;
    esac
    ;;
  *)
    echo "Sistema operativo non supportato: $OS"
    echo "Su Windows usa PowerShell:"
    echo "  irm https://raw.githubusercontent.com/Tnnienn/piadazip/main/install.ps1 | iex"
    exit 1
    ;;
esac

# ── Ultima versione ───────────────────────────────────────────────────────────

LATEST=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
  | grep '"tag_name"' \
  | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')

if [ -z "$LATEST" ]; then
  echo "Errore: impossibile determinare l'ultima versione da GitHub."
  exit 1
fi

# ── Download ──────────────────────────────────────────────────────────────────

ARCHIVE="piadazip-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${LATEST}/${ARCHIVE}"

echo "Installazione piadazip ${LATEST} (${TARGET})..."

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

curl -fsSL --progress-bar "$URL" -o "$TMP/$ARCHIVE"
tar -xzf "$TMP/$ARCHIVE" -C "$TMP"

# ── Installazione ─────────────────────────────────────────────────────────────

INSTALL_DIR="/usr/local/bin"

if [ -w "$INSTALL_DIR" ]; then
  install -m755 "$TMP/$BIN" "$INSTALL_DIR/$BIN"
else
  echo "Installo in $INSTALL_DIR (richiede sudo)..."
  sudo install -m755 "$TMP/$BIN" "$INSTALL_DIR/$BIN"
fi

echo ""
echo "piadazip ${LATEST} installato in $INSTALL_DIR/$BIN"
echo "Prova: piadazip --help"
