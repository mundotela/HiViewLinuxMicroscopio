# HiView — Convenções

> **Versão:** 0.1 · **Data:** 2026-11-10
> Este documento é **obrigatório** para o Devin. Em caso de conflito com qualquer outra fonte (exemplos da internet, hábitos, sugestões de IA), **este documento vence**. Se uma regra impedir algo necessário, pergunte no relatório; não a ignore.

---

## 1. Idioma

| O quê | Idioma |
|---|---|
| Código, identificadores, comentários, mensagens de log | **Inglês** |
| Mensagens de commit, nomes de branch, títulos de PR | **Inglês** |
| Textos da interface (UI) | **Português do Brasil** |
| Documentação em `docs/`, relatórios ao final de milestone | **Português do Brasil** |

Todos os textos da UI ficam centralizados em `crates/hiview-app/src/strings.rs`, como `pub const`. Nenhuma string de UI fica espalhada pelo código, para facilitar a internacionalização futura.

---

## 2. Toolchain e qualidade

- Rust **stable**, edition **2024**. `rust-toolchain.toml` fixa o canal `stable` com `rustfmt` e `clippy`.
- `cargo fmt` com o padrão (sem `rustfmt.toml` customizado).
- `cargo clippy --workspace --all-targets -- -D warnings`: zero warnings.
- No `Cargo.toml` do workspace:
  ```toml
  [workspace.lints.rust]
  unsafe_code = "deny"
  [workspace.lints.clippy]
  unwrap_used = "warn"
  expect_used = "warn"
  ```
  - **Exceção:** `hiview-buttond` pode usar `unsafe` **apenas** no módulo `usbmon_ffi.rs` (ioctl), com `#![allow(unsafe_code)]` local e um comentário `// SAFETY:` em cada bloco.
- `Cargo.lock` versionado (são binários).
- Versões de dependências declaradas em `[workspace.dependencies]` e herdadas com `workspace = true`.

---

## 3. Dependências aprovadas

Use a **versão estável mais recente compatível** com as versões mínimas de sistema (`ARCHITECTURE.md §4`). Qualquer crate **fora** desta lista precisa ser justificado no relatório **antes** de ser adicionado.

| Área | Crates |
|---|---|
| UI | `gtk4` (feature `v4_14`), `libadwaita` (feature `v1_5`), `relm4` |
| Vídeo | `gstreamer`, `gstreamer-app`, `gstreamer-video`, `gstreamer-pbutils` (discoverer) |
| Hardware | `udev`, `v4l`, `evdev`, `nix` ou `libc` (só no buttond) |
| Imagem | `image` (png, jpeg), `cairo-rs` (via gtk4) |
| Config | `serde`, `toml` |
| Erros | `thiserror` (bibliotecas), `anyhow` (binários) |
| Log | `tracing`, `tracing-subscriber` (feature `env-filter`) |
| CLI | `clap` (feature `derive`) |
| Tempo | `chrono` |
| Testes | `tempfile`, `approx` |

**Não use:** `tokio` (o loop do glib/Relm4 basta), `opencv`, `nokhwa`, `egui`, `lazy_static` (use `std::sync::LazyLock`).

---

## 4. Estrutura e nomes

- `snake_case` para módulos, funções e variáveis. `PascalCase` para tipos. `SCREAMING_SNAKE_CASE` para constantes.
- Um módulo por responsabilidade, de preferência com menos de 400 linhas por arquivo. Se passar disso, divida.
- Nomes de tipos de domínio fixos (usar exatamente estes):
  - `Frame`, `FrameBus`;
  - `CameraDescriptor`, `CameraState`, `CameraEvent`;
  - `CapturePipeline`, `CaptureSource`, `Recorder`;
  - `ViewTransform`, `OverlayRenderer`, `OverlayState`;
  - `CalibrationProfile`;
  - `ButtonEvent`, `ButtonSource`;
  - `AppConfig`.
- Layout de `hiview-app`:
  ```
  src/
    main.rs            (só CLI + logging + RelmApp::run)
    app.rs             (AppModel raiz)
    strings.rs
    components/        (um arquivo ou pasta por componente Relm4)
    widgets/viewport/  (mod.rs + imp.rs — subclasse GTK)
    services/          (photo.rs, gallery.rs ...)
    overlay/           (renderer.rs, tools/)
  ```

---

## 5. Erros

- Bibliotecas (`hiview-core`, `hiview-capture`, `hiview-button`): um `enum Error` por crate com `thiserror` e `pub type Result<T> = std::result::Result<T, Error>`.
- Binários: `anyhow::Result` com `.context("...")` em toda fronteira de I/O.
- **Proibido:** `panic!`, `unwrap()`, `expect()`, `unreachable!()` em código de produção. Exceção: invariantes de fato impossíveis, com comentário `// INVARIANT: <por que nunca falha>`.
- Erros que o usuário precisa ver chegam à UI como `adw::Toast` (recuperáveis) ou `adw::StatusPage` (câmera indisponível), **com texto em PT-BR compreensível**. Detalhes técnicos vão para o log, não para o usuário.

---

## 6. Logging

- `tracing` em todo lugar. Nada de `println!`/`eprintln!` (exceto no `check-env.sh`).
- Níveis:
  - `error`: falha que o usuário percebe;
  - `warn`: degradação (frame descartado, fallback de encoder);
  - `info`: ciclo de vida (câmera conectada, gravação iniciada);
  - `debug`: detalhes;
  - `trace`: por frame.
- **Nunca** logar nada por frame em `info` ou acima.

---

## 7. Threads e UI

- A thread principal **nunca** bloqueia. É proibido na thread de UI:
  - I/O de disco;
  - encode de imagem ou vídeo;
  - `std::thread::sleep`;
  - `Mutex` com espera longa;
  - chamadas síncronas ao GStreamer que aguardam mudança de estado.
- Trabalho pesado vai para comandos assíncronos do Relm4 (`sender.oneshot_command` / `spawn_command`) ou para `gio::spawn_blocking`.
- Callbacks do GStreamer (`appsink`, bus em thread) **não tocam em widgets**. Eles enviam mensagens pelo `ComponentSender`/`Sender`.

---

## 8. Padrões Relm4

- Cada componente: `struct Model`, `enum Input`, `enum Output`, `enum CommandOutput` (se houver), e `view!` declarativo.
- Estado da UI **só** no `Model`. Widgets não guardam estado próprio (exceto o `Viewport`, que é uma subclasse GTK com estado de renderização).
- Mensagens no imperativo e específicas: `Input::TakePhoto`, `Input::ToggleFreeze`, `Input::SetZoom(f64)`. Evite nomes como `Input::Update`.
- Atalhos de teclado registrados via `relm4::actions` com accelerators no nível da aplicação, e listados na janela de atalhos.
- O componente raiz **não** contém lógica de domínio: ele delega a serviços e subcomponentes.

---

## 9. Padrões GStreamer

- Pipelines construídos com elementos e `gst::ElementFactory::make(...).property(...).build()`. **Não** use `gst::parse::launch` em produção (só em testes, se ajudar).
- Antes de criar um elemento opcional (`x264enc`, `openh264enc`), verifique com `gst::ElementFactory::find`.
- Todo pipeline tem o bus observado. `Error` e `Eos` são tratados explicitamente.
- `Drop` de qualquer struct dona de pipeline leva o pipeline a `gst::State::Null`.
- Gravação: **sempre** EOS → aguardar `Eos` no bus (com timeout) → `Null`. Ver `ARCHITECTURE.md §6.7`.
- **Nunca** use `/dev/videoN` fixo. O caminho vem sempre de `CameraDescriptor`.

---

## 10. Testes

- `hiview-core` tem cobertura alta, porque é lógica pura. Todo bug corrigido em `hiview-core` ganha um teste de regressão.
- Testes de integração com GStreamer usam `videotestsrc`/fixtures e rodam no CI. Use `gst::init()` uma vez via `LazyLock`.
- Testes que exigem hardware real são marcados com `#[ignore = "requires microscope"]` e documentados no relatório para o Henrique rodar.
- Nomes de teste descrevem o comportamento: `zoom_keeps_point_under_cursor_fixed`, `status_packet_press_is_parsed`.

---

## 11. Git e PR

- Branch: `m<N>-<slug>` (ex.: `m1-live-capture`). Correções no mesmo branch até o merge.
- Commits no formato **Conventional Commits**: `feat(capture): add udev hotplug watcher`, `fix(app): ...`, `test(core): ...`, `docs: ...`, `chore: ...`.
  - Escopos: `core`, `capture`, `button`, `buttond`, `app`, `ci`, `docs`.
- Commits pequenos e coerentes. Não junte refatoração com feature no mesmo commit.
- PR com o título `M<N>: <nome do milestone>` e corpo = relatório (`DEVIN_REPORT_TEMPLATE.md`).
- **Não faça merge sozinho.** A engenharia faz o merge após a aprovação.

---

## 12. Documentação

- Toda mudança visível ao usuário atualiza `docs/USER_MANUAL.md` no mesmo PR.
- Mudança de arquitetura **não** é feita por iniciativa do Devin: ela é proposta no relatório. Se aprovada, a engenharia atualiza o `ARCHITECTURE.md`.
- Itens públicos (`pub`) de bibliotecas têm doc comment `///` curto, em inglês.

---

## 13. UI e UX

- Use widgets do libadwaita sempre que existir um equivalente (`adw::Toast`, `adw::StatusPage`, `adw::PreferencesDialog`, `adw::AlertDialog`, `adw::OverlaySplitView`).
- Siga as GNOME HIG: ícones simbólicos (`*-symbolic`), espaçamentos padrão. Respeite o tema claro/escuro do sistema.
- O modo Explorar deve ser utilizável por quem nunca usou um microscópio: botões grandes, poucos controles, nada de jargão.
- Toda ação tem atalho de teclado e tooltip.
- Conjunto de atalhos (canônico, igual ao manual):

| Tecla | Ação |
|---|---|
| `Espaço` | Tirar foto |
| `R` | Iniciar/parar gravação |
| `F` | Congelar/descongelar |
| `G` | Grade |
| `C` | Mira |
| `L` | Régua (Bancada) |
| `H` / `V` | Espelhar horizontal / vertical |
| `[` / `]` | Girar −90° / +90° |
| `+` / `-` | Zoom |
| `0` / `1` | Ajustar à janela / 100% |
| `M` | Alternar Explorar/Bancada |
| `Ctrl+G` | Galeria |
| `Ctrl+,` | Preferências |
| `F11` / `Esc` | Tela cheia / sair ou cancelar |
| `Ctrl+?` | Atalhos |
| `Ctrl+Q` | Sair |
