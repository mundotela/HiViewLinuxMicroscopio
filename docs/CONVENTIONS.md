# HiView — Convenções

> **Versão:** 0.1 · **Data:** 2026-11-10
> Este documento é **obrigatório** para o Devin. Em caso de conflito com qualquer outra fonte (exemplos da internet, hábitos, sugestões de IA), **este documento vence**. Se uma regra impedir algo necessário, pergunte no relatório; não a ignore.

---

## 1. Idioma

| O quê | Idioma |
|---|---|
| Código, identificadores, comentários, mensagens de log | **Inglês** |
| Mensagens de commit, nomes de branch | **Inglês** |
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
| Hardware | `udev`, `evdev`, `nix` ou `libc` (só no buttond) |
| Imagem | `image` (png, jpeg), `cairo-rs` (via gtk4) |
| Config | `serde`, `toml` |
| Erros | `thiserror` (bibliotecas), `anyhow` (binários) |
| Log | `tracing`, `tracing-subscriber` (feature `env-filter`) |
| CLI | `clap` (feature `derive`) |
| Tempo | `chrono` |
| Testes | `tempfile`, `approx` |

**Não use:** `tokio` (o loop do glib/Relm4 basta), `v4l` (exige libclang; enumeração e formatos vêm de udev + `gst::DeviceMonitor`), `opencv`, `nokhwa`, `egui`, `lazy_static` (use `std::sync::LazyLock`).

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
- Todo arquivo `.rs` começa com o cabeçalho SPDX: `// SPDX-License-Identifier: GPL-3.0-or-later` e `// Copyright (C) 2026 MundoTela Tecnologia`.

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
- Contadores e métricas de diagnóstico (ex.: instâncias vivas, tempos de miniatura) ficam em `debug`.
- **Ganchos por variável de ambiente para testes ou benchmarks não ficam no código de produção.** Antes do relatório, rode `git grep -n "env::var" -- crates/`: toda ocorrência precisa ser legítima (ex.: `XDG_CONFIG_HOME`) e estar justificada no relatório.

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
- **Nunca segure um `borrow` de `RefCell`/`Mutex` durante uma chamada ao GTK ou ao GStreamer.** Copie o que precisa, solte o borrow e só então chame. O GTK pode reentrar no nosso código pela mesma pilha, e um pânico dentro de um callback do GTK não se recupera: o app **aborta** (crash do M5). Em callbacks de factory (`setup`/`bind`/`unbind`/`teardown`), use `try_borrow` e adie com `glib::idle_add_local_once` em vez de entrar em pânico.
- **Widgets reciclados (`GridView`/`ListView`):** nunca guarde referência a um widget de item, nem pendure popover ou diálogo nele. Resolva o item pelo **dado do modelo** (ex.: o caminho do arquivo) no momento da ação, e apresente diálogos na janela. Widgets de item não devem ser alvo do ponteiro (`can_target=false`): o GTK 4.14 tem um bug com widgets desparentados sob o cursor (`Gtk-CRITICAL gtk_widget_compute_point`). Gestos e menus ficam na grade, que é permanente.
- **Threads externas → UI:** callbacks de worker ou de GStreamer enviam mensagens com `input_sender().send(...)`, **descartando o erro**. Nunca use `sender.input(...)` nesses callbacks: ele entra em pânico se a UI já foi encerrada (bug encontrado no M6, presente desde o M2, que aparecia ao fechar o app com a câmera transmitindo).
- **A roda do mouse nunca muda o valor de um controle.** `adw::ComboRow`/`GtkDropDown`, `GtkScale` (deslizantes) e `adw::SpinRow`/`GtkSpinButton` mudam de valor com a roda; rolar o painel por cima deles reescrevia a configuração sem o usuário perceber (no M8, o perfil de calibração ativo "sumia" assim). Todo controle desses leva um `EventControllerScroll` em fase capture que engole a roda e repassa o deslocamento ao `ScrolledWindow` ancestral (`measure_ui::swallow_scroll`). Rolar o painel só rola o painel.
- **Foco do teclado:** os atalhos de uma tecla (G, C, M, Espaço…) não funcionam enquanto um campo editável tem o foco. Depois de qualquer diálogo ou popover fechar, ou quando o usuário termina de editar um campo (Enter ou ponteiro saindo do painel), o foco volta para a viewport. Clicar no vídeo sempre devolve o foco à viewport. Botões que não precisam de foco (seletores de modo, Galeria) usam `focusable=false`.
- **Relógios: nunca misture domínios.** Timestamps vindos do kernel não são comparáveis com o relógio do app: o usbmon usa CLOCK_REALTIME, o evdev usa CLOCK_MONOTONIC e `glib::monotonic_time`/`Instant` usam CLOCK_MONOTONIC. Use um timestamp externo **só para deltas da mesma fonte** (ex.: soltar − apertar). Timers e prazos no app são agendados pelo relógio de **chegada** (`timeout_add_local_once(duração)`). No M9, a mistura fez o aperto longo nunca disparar.
- **Avisos de status (toasts) só na mudança de estado.** Compare com o estado anterior no modelo; tentativas repetidas (reconexão, polling) são silenciosas (`debug!`). A primeira detecção também conta como mudança. Cada transição tem uma linha `info!` no log (M9: enxurrada de toasts com o serviço parado).
- **Critério de aceite de UI:** zero linhas `Gtk-CRITICAL`, `Gdk-CRITICAL` ou `panicked` no log de uma sessão completa de uso.

---

## 8. Padrões Relm4

- Cada componente: `struct Model`, `enum Input`, `enum Output`, `enum CommandOutput` (se houver), e `view!` declarativo.
- Estado da UI **só** no `Model`. Widgets não guardam estado próprio (exceto o `Viewport`, que é uma subclasse GTK com estado de renderização).
- Mensagens no imperativo e específicas: `Input::TakePhoto`, `Input::ToggleFreeze`, `Input::SetZoom(f64)`. Evite nomes como `Input::Update`.
- Atalhos de teclado: ações `win.*` via `relm4::actions`, disparadas por um `EventControllerKey` (fase capture) na janela. **Nunca** use `set_accelerators_for_action` no nível da aplicação: ele dispara dentro de diálogos e rouba as teclas digitadas em campos de texto (bug do M8). O mapa tecla→ação é a função `shortcut_action` (fonte única da §13), e o gate `focus::shortcuts_allowed` bloqueia tudo com diálogo aberto ou campo editável focado. Os atalhos ficam listados na janela de atalhos.
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

## 11. Git e relatórios

- Branch **local**: `m<N>-<slug>` (ex.: `m1-live-capture`), criado a partir de `main`. Correções no mesmo branch até o merge.
- Commits no formato **Conventional Commits**: `feat(capture): add udev hotplug watcher`, `fix(app): ...`, `test(core): ...`, `docs: ...`, `chore: ...`.
  - Escopos: `core`, `capture`, `button`, `buttond`, `app`, `ci`, `docs`.
- Commits pequenos e coerentes. Não junte refatoração com feature no mesmo commit.
- **Verificação final sobre o que está commitado:** antes de escrever o relatório, a árvore de trabalho precisa estar limpa (`git status --short` vazio, exceto arquivos que a engenharia deixou pendentes de propósito). Só então rode fmt, clippy e testes. Cole a saída de `git status --short` e `git log --oneline main..HEAD` no relatório. **`git worktree list` deve mostrar só a pasta do projeto**: cópias de trabalho temporárias (ex.: para medir a linha de base no `main`) são removidas antes do relatório, sem deixar ganchos de medição nelas (no M7, uma cópia esquecida em `/tmp` com um gancho de gravação automática bloqueou o merge). Motivo: no M1, um commit foi gravado com conteúdo intermediário do editor e entrou em `main` quebrado, sem a CI detectar.
- Ao concluir: relatório em `docs/reports/M<N>.md` (seguindo `DEVIN_REPORT_TEMPLATE.md`), commitado no branch e também colado no chat.
- **Proibido:** `git push`, abrir PR, fazer merge em `main`, reescrever histórico (`rebase`/`reset --hard`/`push --force`) de `main`. Tudo isso é papel da engenharia.

## 11.0 Medições e testes manuais

- Toda medição de CPU/fps citada em relatório é feita em **build release** (`cargo run --release`), **na sessão Wayland nativa** (não em XWayland/`GDK_BACKEND=x11`, que acrescenta ~10–14 pontos ao mesmo binário), sem ferramentas de captura de tela ou injeção de teclas rodando durante a amostragem, e diz isso explicitamente. Números em debug só valem como informação adicional.
- Uma instância do app entregue ao Henrique para teste manual **nunca** roda com `timeout` nem é encerrada pelo Devin sem aviso. Se precisar de uma instância temporária para medição, use outra janela/instância e avise.

## 11.1 Segurança na máquina local

- **Nunca** rode `sudo`, `apt`, `pip install --user` ou qualquer comando que altere o sistema, nem escreva fora da pasta do projeto (exceção: `~/.cargo`, que o próprio cargo gerencia).
- Se faltar um pacote do sistema, **pare e peça** no chat, informando o comando exato para o Henrique rodar.
- Acesso ao microscópio (`/dev/video*`) é permitido. Não altere controles V4L2 de forma persistente fora do app.
- **Código que roda como root** (hoje: `hiview-buttond`, `scripts/install-buttond.sh`, a unit systemd) passa por revisão de segurança da engenharia, **linha a linha, com os arquivos completos**, antes de qualquer `sudo`. Qualquer mudança nesses arquivos exige nova revisão e reinstalação; mudanças só no app não.

---

## 12. Documentação

- Toda mudança visível ao usuário atualiza `docs/USER_MANUAL.md` no mesmo branch do milestone.
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
| `L` / `A` / `O` | Régua / ângulo / círculo (Bancada) |
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
