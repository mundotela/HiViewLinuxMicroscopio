#!/usr/bin/env bash
# check-env.sh — verify that the build environment meets HiView's minimums.
# Minimums (docs/ARCHITECTURE.md §4): GTK 4.14, libadwaita 1.5, GStreamer 1.24.
# The rustc minimum is read from `rust-version` in the workspace Cargo.toml
# (single source of truth), not hardcoded here.
#
# echo/printf are allowed here by convention (docs/CONVENTIONS.md §6).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

MIN_RUSTC="$(sed -n 's/^rust-version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' \
    "$REPO_ROOT/Cargo.toml" | head -n1)"
if [ -z "$MIN_RUSTC" ]; then
    echo "ERRO: rust-version não encontrado em $REPO_ROOT/Cargo.toml" >&2
    exit 1
fi
MIN_GTK="4.14"
MIN_ADW="1.5"
MIN_GST="1.24"

status=0

version_ge() {
    # version_ge ACTUAL MINIMUM — true if ACTUAL >= MINIMUM
    [ "$(printf '%s\n%s\n' "$2" "$1" | sort -V | head -n1)" = "$2" ]
}

report() {
    # report NAME ACTUAL MINIMUM
    local name="$1" actual="$2" minimum="$3"
    if version_ge "$actual" "$minimum"; then
        printf '  %-16s %-10s (mínimo %s)  OK\n' "$name" "$actual" "$minimum"
    else
        printf '  %-16s %-10s (mínimo %s)  FALHOU\n' "$name" "$actual" "$minimum"
        status=1
    fi
}

check_pkg() {
    # check_pkg NAME PKG-CONFIG-MODULE MINIMUM
    local name="$1" module="$2" minimum="$3" actual
    if ! command -v pkg-config >/dev/null 2>&1; then
        printf '  %-16s pkg-config não encontrado  FALHOU\n' "$name"
        status=1
        return
    fi
    if ! actual="$(pkg-config --modversion "$module" 2>/dev/null)"; then
        printf '  %-16s não encontrado (módulo %s)  FALHOU\n' "$name" "$module"
        status=1
        return
    fi
    report "$name" "$actual" "$minimum"
}

echo "HiView — verificação de ambiente"
echo "---------------------------------"

if command -v rustc >/dev/null 2>&1; then
    report "rustc" "$(rustc --version | awk '{print $2}')" "$MIN_RUSTC"
else
    printf '  %-16s não encontrado  FALHOU\n' "rustc"
    status=1
fi

check_pkg "GTK (gtk4)" "gtk4" "$MIN_GTK"
check_pkg "libadwaita" "libadwaita-1" "$MIN_ADW"
check_pkg "GStreamer" "gstreamer-1.0" "$MIN_GST"

echo "---------------------------------"
if [ "$status" -eq 0 ]; then
    echo "Ambiente OK."
else
    echo "Ambiente INCOMPLETO — veja os itens que falharam acima."
fi
exit "$status"
