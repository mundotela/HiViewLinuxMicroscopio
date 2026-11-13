#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# HiView — instalador do daemon do botão Snap (docs/ARCHITECTURE.md §6.10).
#
# Uso:   cargo build --release -p hiview-buttond     (como usuário normal)
#        sudo ./scripts/install-buttond.sh           instala e ativa
#        sudo ./scripts/install-buttond.sh --uninstall   reverte tudo
#
# Idempotente: cada etapa verifica antes de agir e pode ser repetida.
# O script NÃO compila: um build via sudo deixaria target/ com dono root.

set -euo pipefail

BIN_NAME="hiview-buttond"
UNIT_NAME="hiview-buttond.service"
GROUP="hiview"
BIN_SRC="target/release/$BIN_NAME"
BIN_DST="/usr/local/libexec/$BIN_NAME"
UNIT_DST="/etc/systemd/system/$UNIT_NAME"
MODLOAD_DST="/etc/modules-load.d/hiview.conf"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
UNIT_SRC="$ROOT/data/hiview-buttond.service"
MODLOAD_SRC="$ROOT/data/modules-load.d/hiview.conf"

say() { printf '» %s\n' "$*"; }
ok() { printf '  ✓ %s\n' "$*"; }

# Apenas via `sudo` a partir de uma sessão de usuário: recusa um shell
# root direto (não haveria usuário para receber o grupo) e nunca
# adiciona o próprio root ao grupo.
require_invocation() {
    if [[ $EUID -ne 0 ]]; then
        say "Este script precisa de root. Rode: sudo $0 ${1:-}"
        exit 1
    fi
    if [[ -z ${SUDO_USER:-} || $SUDO_USER == root ]]; then
        say "Rode 'sudo $0' a partir do seu usuário de sessão — nunca"
        say "num shell root direto (o grupo '$GROUP' precisa receber o"
        say "usuário que usa o HiView, e root nunca entra nele)."
        exit 1
    fi
}

# O binário é compilado pelo usuário, não dentro deste script.
require_binary() {
    if [[ ! -x $ROOT/$BIN_SRC ]]; then
        say "Binário ausente: $ROOT/$BIN_SRC"
        say "Compile antes, como o seu usuário normal:"
        say "    cargo build --release -p $BIN_NAME"
        exit 1
    fi
}

CALLER="$SUDO_USER"

install_files() {
    say "Instalando binário em $BIN_DST…"
    install -D -m 0755 "$ROOT/$BIN_SRC" "$BIN_DST"
    ok "binário instalado"

    say "Instalando unit $UNIT_NAME…"
    install -D -m 0644 "$UNIT_SRC" "$UNIT_DST"
    ok "unit instalada"

    say "Instalando $MODLOAD_DST (carrega usbmon no boot)…"
    install -D -m 0644 "$MODLOAD_SRC" "$MODLOAD_DST"
    ok "modules-load instalado"
}

setup_group() {
    if getent group "$GROUP" >/dev/null; then
        ok "grupo '$GROUP' já existe"
    else
        say "Criando grupo '$GROUP'…"
        groupadd --system "$GROUP"
        ok "grupo criado"
    fi

    if id -nG "$CALLER" | tr ' ' '\n' | grep -qx "$GROUP"; then
        ok "$CALLER já pertence ao grupo '$GROUP'"
    else
        say "Adicionando $CALLER ao grupo '$GROUP'…"
        usermod -aG "$GROUP" "$CALLER"
        ok "usuário adicionado — faça logout/login para valer"
    fi
}

load_module() {
    if lsmod | grep -q '^usbmon'; then
        ok "usbmon já carregado"
    else
        say "Carregando módulo usbmon…"
        modprobe usbmon
        ok "usbmon carregado"
    fi
}

start_service() {
    say "Verificando e ativando o serviço…"
    systemd-analyze verify "$UNIT_DST" || {
        say "ERRO: systemd-analyze verify reprovou a unit"
        exit 1
    }
    systemctl daemon-reload
    systemctl enable --now "$UNIT_NAME"
    systemctl --no-pager --full status "$UNIT_NAME" | head -5 || true
}

uninstall() {
    say "Parando e desativando $UNIT_NAME…"
    systemctl disable --now "$UNIT_NAME" 2>/dev/null || true
    ok "serviço parado (se estava ativo)"

    say "Removendo arquivos instalados…"
    rm -f "$UNIT_DST" "$MODLOAD_DST" "$BIN_DST"
    ok "unit, modules-load e binário removidos"

    if id -nG "$CALLER" 2>/dev/null | tr ' ' '\n' | grep -qx "$GROUP"; then
        say "Removendo $CALLER do grupo '$GROUP'…"
        gpasswd -d "$CALLER" "$GROUP" >/dev/null
        ok "usuário removido do grupo"
    fi

    if getent group "$GROUP" >/dev/null; then
        say "Removendo grupo '$GROUP'…"
        if ! groupdel "$GROUP" 2>/dev/null; then
            say "  (grupo não removido — é grupo primário de alguém;"
            say "   revise com 'getent group $GROUP')"
        else
            ok "grupo removido"
        fi
    fi

    systemctl daemon-reload
    ok "systemd recarregado. O módulo usbmon permanece carregado"
    ok "(inofensivo — usado por outras ferramentas de depuração USB)."
    say "Desinstalação concluída. Faça logout/login para limpar o grupo."
}

case "${1:-install}" in
    install)
        require_invocation "$@"
        require_binary
        install_files
        setup_group
        load_module
        start_service
        say "Pronto! Faça logout/login para o grupo valer, depois abra o HiView."
        ;;
    --uninstall | uninstall)
        require_invocation "$@"
        uninstall
        ;;
    *)
        say "Uso: sudo $0 [--uninstall]"
        exit 2
        ;;
esac
