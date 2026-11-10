# HiView

Aplicativo de desktop para Linux (GTK4 + libadwaita + Relm4 + GStreamer) para
microscópios USB do padrão UVC — ver, fotografar, gravar e medir.

Documentação completa em [`docs/`](docs/): arquitetura, convenções, milestones
e manual do usuário.

## Requisitos de sistema

Pop!_OS / Ubuntu 24.04 ou superior, com:

- GTK **4.14** ou superior
- libadwaita **1.5** ou superior
- GStreamer **1.24** ou superior
- Rust **stable** (edition 2024), via [rustup](https://rustup.rs)

No Pop!_OS 24.04 / Ubuntu 24.04:

```bash
sudo apt install libgtk-4-dev libadwaita-1-dev \
    libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
    gstreamer1.0-plugins-good gstreamer1.0-plugins-bad \
    gstreamer1.0-plugins-ugly gstreamer1.0-tools
```

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

A definir pela engenharia (ver `docs/ARCHITECTURE.md` §12).
