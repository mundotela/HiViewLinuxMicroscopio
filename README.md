# HiView

Aplicativo de desktop para Linux (GTK4 + libadwaita + Relm4 + GStreamer) para
microscópios USB do padrão UVC — ver, fotografar, gravar e medir.

Documentação completa em [`docs/`](docs/): arquitetura, convenções, milestones
e manual do usuário.

## Instalação (pacotes .deb)

A forma recomendada para o usuário final é o pacote Debian — ele já
declara todas as dependências de runtime e instala ícone, `.desktop`
e metainfo:

```bash
sudo apt install ./hiview_1.0.0-1_amd64.deb            # aplicativo
sudo apt install ./hiview-buttond_1.0.0-1_amd64.deb    # opcional: botão Snap
```

Após instalar o `hiview-buttond`, faça **logout/login** para liberar o
acesso ao botão. O manual do usuário (`docs/USER_MANUAL.md` §3) detalha
desinstalação e migração de instalações feitas pelo script.

## Gerando os pacotes

Requer `cargo-deb` (`cargo install cargo-deb`) e as dependências de
sistema abaixo:

```bash
cargo deb -p hiview-app
cargo deb -p hiview-buttond
# Saída em target/debian/*.deb — inspecione com dpkg-deb -I / -c e lintian
```

O pacote do daemon é gerado a partir de `crates/hiview-buttond/debian/`
(unit, sysusers, maintainer scripts). **Não instale sem a revisão de
segurança da engenharia** (`docs/CONVENTIONS.md` §11.1).

## Requisitos de sistema

Pop!_OS / Ubuntu 24.04 ou superior, com:

- Processador **x86-64 com SSE4.2** (Intel a partir de ~2009, AMD a
  partir de ~2011) — o build de release compila com
  `target-cpu=x86-64-v2` (`.cargo/config.toml`), usado pelo passe de
  ajuste de imagem do M7
- GTK **4.14** ou superior
- libadwaita **1.5** ou superior
- GStreamer **1.24** ou superior
- Rust **stable** (edition 2024), via [rustup](https://rustup.rs)

No Pop!_OS 24.04 / Ubuntu 24.04:

```bash
sudo apt install libgtk-4-dev libadwaita-1-dev \
    libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
    gstreamer1.0-plugins-good gstreamer1.0-plugins-bad \
    gstreamer1.0-plugins-ugly gstreamer1.0-tools libudev-dev
```

`gstreamer1.0-plugins-ugly` fornece o `x264enc` (encoder H.264 da gravação)
e `gstreamer1.0-plugins-bad` fornece `openh264enc`/`openh264dec`,
`mp4mux`, `h264parse` e `qtdemux`. Sem encoder H.264 o botão de gravar
fica desabilitado.

Para verificar se o ambiente está completo:

```bash
./scripts/check-env.sh
```

## Build e execução

```bash
cargo build                 # compila o workspace
cargo run -p hiview-app     # abre o aplicativo
```

Opções de linha de comando:

```bash
cargo run -p hiview-app -- --source=device          # microscópio real (padrão)
cargo run -p hiview-app -- --source=test            # videotestsrc, sem hardware
cargo run -p hiview-app -- --source=file:video.mp4  # reproduz um arquivo em loop
```

Os logs usam `tracing` e respeitam `RUST_LOG` (padrão `info`):

```bash
RUST_LOG=debug cargo run -p hiview-app
```

## Qualidade

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Estrutura

```
crates/
  hiview-core      lógica pura (sem GTK/GStreamer)
  hiview-capture   descoberta udev, pipeline GStreamer, gravador, V4L2
  hiview-button    abstração do botão físico Snap
  hiview-buttond   helper privilegiado (systemd) do botão Snap
  hiview-app       interface Relm4/GTK4/libadwaita (binário `hiview`)
```

## Licença

HiView é software livre, licenciado sob a GNU GPL v3 ou posterior. Veja LICENSE.
