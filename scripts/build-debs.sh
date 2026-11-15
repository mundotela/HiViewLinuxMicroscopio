#!/usr/bin/env bash
# build-debs.sh — empacota hiview (+dbgsym) e hiview-buttond.
# O MESMO script roda no notebook e no CI (.github/workflows/ci.yml,
# job packaging) — nenhum passo manual de strip: o binário do
# buttond já sai stripped do build via
# [profile.release.package.hiview-buttond] strip = "symbols" no
# Cargo.toml do workspace e o dbgsym do hiview vem do
# `dbgsym = true` no [package.metadata.deb] dele.
#
# Uso: ./scripts/build-debs.sh          (rode da raiz do repositório)
# Saída: target/debian/*.deb + target/debian/*.ddeb
#
# echo/printf são permitidos aqui por convenção (docs/CONVENTIONS.md §6).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# cargo deb sem --no-build já compila em release o pacote pedido;
# o build explícito antes cobre o workspace inteiro uma única vez.
cargo build --release
cargo deb -p hiview-app
cargo deb -p hiview-buttond

# Erros (E:) reprovam o build nos .deb; o .ddeb gerado pelo
# cargo-deb não carrega changelog/copyright próprios, então ele é
# apenas informativo aqui (documentado em docs/reports/M12.md).
lintian --fail-on error target/debian/*.deb
echo "--- lintian (informativo) no .ddeb ---"
lintian target/debian/*.ddeb || true

ls -l target/debian/
