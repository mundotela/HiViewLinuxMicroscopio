# HiView — Arquitetura

> **Versão:** 0.1 · **Data:** 2026-11-10 · **Status:** aprovado para início do M0
> **Engenharia:** Henrique (MundoTela Tecnologia) e Claude · **Implementação:** Devin
> Documentos irmãos: `MILESTONES.md`, `CONVENTIONS.md`, `USER_MANUAL.md`, `DEVIN_REPORT_TEMPLATE.md`

---

## 1. Visão geral

HiView é um aplicativo desktop para Linux que permite **ver, fotografar, gravar e medir** usando microscópios USB do padrão UVC.

Ele atende dois públicos na mesma interface:

| Modo | Público | O que mostra |
|---|---|---|
| **Explorar** | Curiosos, estudantes, crianças com supervisão | Imagem grande, botões grandes de foto/vídeo, zoom, galeria |
| **Bancada** | Quem trabalha com eletrônica (SMD, solda, inspeção de placas) | Tudo do Explorar + grade, mira, medição calibrada, ajustes finos de imagem, congelar quadro |

As funcionalidades do app Android do fabricante foram todas mapeadas (seção 11). Algumas foram reinterpretadas, porque não fazem sentido no desktop.

---

## 2. Hardware alvo — fatos medidos (2026-11-10)

Medições feitas na máquina de desenvolvimento (Pop!_OS, kernel `7.1.5-76070105-generic`).

| Item | Valor medido | Consequência para o projeto |
|---|---|---|
| USB ID | `05e3:f12a` — Genesys Logic, Inc. "Digital Microscope" | Identificação **por VID:PID**, nunca por nome |
| Nome V4L2 | `HD camera` (genérico) | Não serve para identificar o aparelho |
| Nós V4L2 | `/dev/video2` (captura), `/dev/video3` (metadados), `/dev/media1` | Os números **mudam** conforme a ordem em que se conecta. Escolher o nó com capacidade `VIDEO_CAPTURE` |
| Outra câmera no sistema | Webcam Acer `0408:403a` em `/dev/video0` | O app nunca deve assumir `/dev/video0` |
| Formato | **Único:** `YUYV 4:2:2`, **640×480** | Não existe troca real de resolução |
| Taxas | 30 fps e 5 fps | 30 fps é o padrão. 5 fps fica reservado (timelapse, no futuro) |
| "2.0 MP" / "1600X" | Marketing: interpolação/ampliação digital | Resolução maior só por interpolação, **sempre rotulada como tal** |
| Anel de ampliação/foco | Mecânico, sem leitura por software | Zoom do app é só digital. Calibração de medição fica inválida quando o anel é mexido |
| LEDs | Controle por roda no cabo (provavelmente analógico, **a confirmar**) | Sem controle de LED no software, a menos que `--list-ctrls` mostre o contrário |
| Endpoint de status | `EP 0x83 IN`, Interrupt | Canal do botão Snap |
| Descritor de still | `bStillCaptureMethod=2`, `bTriggerSupport=1`, `bTriggerUsage=0` | Ver seção 2.1 |

### 2.1 Botão Snap — comportamento observado

Captura feita com `usbmon` com o stream aberto. Cada aperto gera **dois** pacotes de status UVC de 4 bytes no endpoint 0x83:

```
C Ii:3:008:3 0:16 4 = 02010001   ← pressionado
C Ii:3:008:3 0:16 4 = 02010000   ← solto (≈160 ms depois)
```

| Byte | Valor | Campo UVC | Significado |
|---|---|---|---|
| 0 | `0x02` | `bStatusType` | Origem: interface VideoStreaming |
| 1 | `0x01` | `bOriginator` | Interface nº 1 |
| 2 | `0x00` | `bEvent` | Evento de botão |
| 3 | `0x01` / `0x00` | `bValue` | Pressionado / solto |

**Fatos importantes:**

1. O kernel tem `CONFIG_USB_VIDEO_CLASS_INPUT_EVDEV=y`, mas **não cria** dispositivo de entrada para este aparelho. Hipótese (consistente com tudo que observamos): o `uvcvideo` recente só cria o input device quando `bTriggerSupport=1` **e** `bTriggerUsage=1`. Este aparelho declara `bTriggerUsage=0`, então os pacotes chegam ao driver e são descartados.
2. O endpoint de interrupção **só é consultado enquanto o stream está aberto**. Para o app isso basta, porque o botão só precisa funcionar com o app aberto.
3. O app **não pode** ler o EP 0x83 por libusb, porque isso exigiria desligar o `uvcvideo` e o vídeo pararia.

A solução está na seção 6.10.

---

## 3. Decisões de arquitetura (ADRs resumidas)

| ID | Decisão | Motivo |
|---|---|---|
| ADR-001 | **Rust em todo o projeto** | Uma linguagem só, binário único, segurança de memória, bom suporte a GStreamer/GTK |
| ADR-002 | **GTK4 + libadwaita + Relm4** na interface | Visual nativo e moderno no GNOME/Pop!_OS. O Relm4 impõe uma estrutura de componentes e mensagens que limita código bagunçado |
| ADR-003 | **GStreamer** para captura e gravação. Controles V4L2 pelo crate `v4l` | Pipeline maduro, encoder H.264 pronto, MP4 correto |
| ADR-004 | Dispositivo identificado por **VID:PID via udev**, com hotplug | Os nós `/dev/videoN` mudam, e o nome V4L2 é genérico |
| ADR-005 | Frames chegam à aplicação por **`appsink` em RGBA**, e não por `gtk4paintablesink` | Congelar, foto, empilhamento e medição precisam de acesso ao frame. 640×480×4 B a 30 fps ≈ 37 MB/s, custo irrelevante |
| ADR-006 | Gravação num **pipeline separado alimentado por `appsrc`** | Evita adicionar e remover ramos de `tee` dinamicamente, a fonte nº 1 de bugs com GStreamer. Encerramento controlado por EOS |
| ADR-007 | Botão físico via **helper privilegiado (`hiview-buttond`)** que lê `usbmon`. Se um dia o kernel expuser `KEY_CAMERA`, o app usa `evdev` direto | Única forma de ler o botão no kernel atual sem patch de driver |
| ADR-008 | **Zoom digital só altera a visualização.** Foto e vídeo salvam o quadro inteiro | Comportamento previsível. "Salvar só a área visível" é opção explícita |
| ADR-009 | Resoluções acima de 640×480 são **interpoladas e rotuladas como tal** na UI e no manual | Honestidade com o usuário técnico |
| ADR-010 | Distribuição inicial por **`.deb`**. Flatpak fica para depois | O helper root e as regras de udev complicam o sandbox |
| ADR-011 | Espelhar e girar são aplicados **no pipeline** (`videoflip`) | Tela, foto e vídeo ficam sempre iguais, como no app Android |
| ADR-012 | Sobreposições (grade, mira, medidas) desenhadas por **um único renderizador cairo**, usado tanto na tela quanto ao "gravar sobreposições na foto" | Um só código para os dois usos, sem divergência entre tela e arquivo |

---

## 4. Plataforma alvo

- **SO:** Pop!_OS / Ubuntu 24.04 ou superior. Versões mínimas: **GTK 4.14, libadwaita 1.5, GStreamer 1.24**.
  - ✅ **Confirmado na máquina do Henrique (2026-11-10):** `libgtk-4-1 4.14.5`, `libadwaita-1-0 1.5.0`, `libgstreamer1.0-0 1.24.2`. Ou seja, a máquina está **exatamente no mínimo**. Features dos crates fixadas em `gtk4/v4_14` e `libadwaita/v1_5`. **Não usar APIs mais novas**, mesmo que o ambiente do Devin as tenha.
- **Ambiente do Devin:** o Devin roda como **Devin Local na própria máquina alvo**. O ambiente de desenvolvimento é o ambiente de produção, então não há divergência de versões.
- **Toolchain:** Rust stable, edition 2024.

---

## 5. Estrutura de componentes

Workspace Cargo com 5 crates:

```
hiview/
├── Cargo.toml                (workspace)
├── crates/
│   ├── hiview-core/          lógica pura, SEM GTK/GStreamer
│   │     config, calibração, geometria/transformações de coordenadas,
│   │     processamento de frame (média de quadros, upscale), nomes de arquivo,
│   │     parser de pacote de status UVC
│   ├── hiview-capture/       hardware de vídeo
│   │     descoberta udev, pipeline de captura, FrameBus, gravador (appsrc),
│   │     controles V4L2
│   ├── hiview-button/        abstração do botão físico
│   │     trait ButtonSource, EvdevSource, HelperSource (cliente do socket)
│   ├── hiview-buttond/       BINÁRIO helper root (systemd)
│   │     lê /dev/usbmonN, filtra o EP 0x83 do 05e3:f12a, publica eventos no socket
│   └── hiview-app/           BINÁRIO `hiview` — interface Relm4/GTK4/libadwaita
│         componentes, widget Viewport, renderizador de sobreposições, galeria
├── data/                     .desktop, ícones, metainfo, regra udev, unit systemd
├── docs/                     estes documentos
└── tests/fixtures/           vídeo/fotos reais do microscópio para testes sem hardware
```

**Regra de dependência:** `hiview-app` → `hiview-capture`, `hiview-button`, `hiview-core`. `hiview-buttond` → `hiview-core`. `hiview-core` não depende de nenhum outro crate do projeto, nem de GTK ou GStreamer.

```
┌──────────────────────────── hiview-app (GTK main thread) ───────────────────────────┐
│  AppModel (Relm4)                                                                    │
│   ├─ HeaderBar / Toolbar  ├─ Viewport (widget próprio)  ├─ SidePanel (Bancada)       │
│   ├─ Gallery              └─ Preferences                                             │
└───────▲──────────────────────────────▲─────────────────────────────▲─────────────────┘
        │ FrameReady (coalescido)       │ CameraEvent                 │ ButtonEvent
┌───────┴────────────── hiview-capture ─┴──────────┐        ┌─────────┴─────── hiview-button ─┐
│ DeviceWatcher (udev) → CapturePipeline           │        │ EvdevSource │ HelperSource        │
│ v4l2src→caps→videoconvert→videoflip→appsink      │        └─────────────┬───────────────────┘
│            │                                     │                      │ unix socket
│         FrameBus ──► Recorder (appsrc→x264→mp4)  │        ┌─────────────┴─────── hiview-buttond (root) ┐
│ V4l2Controls (crate v4l, fd separado)            │        │ /dev/usbmonN → filtro → eventos            │
└──────────────────────────────────────────────────┘        └────────────────────────────────────────────┘
```

---

## 6. Componentes em detalhe

### 6.1 Descoberta de dispositivo (`hiview-capture::device`)

- Enumera o subsistema `video4linux` via crate `udev`. Para cada nó, sobe até o pai `usb_device` e lê `idVendor`/`idProduct`.
- Filtra os nós com capacidade `V4L2_CAP_VIDEO_CAPTURE` no `device_caps`. Isso descarta o `/dev/video3`, que é de metadados.
- **Seleção automática:** prioridade para `05e3:f12a`. Se não houver, a lista mostra todas as câmeras UVC e o usuário escolhe. Uma tabela `KNOWN_MICROSCOPES` em `hiview-core` permite adicionar outros modelos no futuro.
- **Hotplug:** `udev::MonitorBuilder` no subsistema `video4linux`. Eventos `add`/`remove` viram `CameraEvent::Connected/Disconnected`.
- O resultado é um `CameraDescriptor { vid, pid, name, dev_node, bus_num, dev_num, usb_path }`. `bus_num`/`dev_num` também são usados pelo helper do botão.

### 6.2 Pipeline de captura (`hiview-capture::pipeline`)

```
v4l2src device=<dev_node> do-timestamp=true
  ! video/x-raw,format=YUY2,width=640,height=480,framerate=30/1
  ! videoconvert
  ! video/x-raw,format=RGBA
  ! videoflip name=flip video-direction=identity
  ! appsink name=sink sync=false max-buffers=1 drop=true emit-signals=false
```

- As caps vêm de `CaptureMode`, construído a partir do que o dispositivo anuncia (`VIDIOC_ENUM_FMT`). **Nada fica fixo no código além do padrão.**
- Callback `new-sample` (thread de streaming do GStreamer): copia o buffer para um `Frame` e publica no `FrameBus`.
- `Frame { seq: u64, pts: gst::ClockTime, width: u32, height: u32, stride: u32, data: glib::Bytes }`, sempre RGBA8.
- O **bus** do pipeline é observado. `Error` derruba o estado para `CameraState::Error(msg)` e leva o pipeline a `Null`. Sem `panic`.
- Quando o dispositivo some no meio do stream, vai para o estado `Disconnected` e reconecta sozinho quando o udev anunciar o aparelho de volta.

**Máquina de estados da câmera:**

```
Searching ──found──► Starting ──PLAYING──► Streaming
    ▲                   │                     │
    │                 error                remove/error
    └──── Disconnected ◄┴─────────────────────┘
```

> "Congelado" **não** é estado da câmera. É estado da visualização: a câmera continua capturando.

### 6.3 FrameBus

- `latest: Mutex<Option<Arc<Frame>>>`: o quadro mais recente.
- **UI:** um `AtomicBool ui_pending`. Ao publicar, se `ui_pending` estiver falso, ele vira verdadeiro e uma mensagem `FrameReady` é enviada à UI. A UI pega o `latest` e zera o flag. Assim **a fila nunca acumula**: se a UI atrasar, quadros são pulados.
- **Assinantes completos** (gravador, empilhador): `std::sync::mpsc::sync_channel(8)`. Se o canal estiver cheio, o quadro é descartado e um contador `dropped_frames` é incrementado (com log em `warn`).

### 6.4 Transformações

- Espelhar H/V e girar ±90°/180° alteram a propriedade `video-direction` do `videoflip`, em tempo real.
- Girar 90° muda as dimensões para 480×640. O `Frame` carrega `width`/`height`, então tudo abaixo se adapta.
- **Durante a gravação, girar fica bloqueado** (a UI desabilita o botão), porque a resolução de um vídeo não pode mudar no meio. Espelhar continua permitido.

### 6.5 Viewport (`hiview-app::widgets::viewport`)

- Widget próprio (subclasse de `gtk::Widget`) que implementa `snapshot()`:
  1. `gdk::MemoryTexture` criado do `Frame` (`MemoryFormat::R8g8b8a8`, sem cópia graças ao `glib::Bytes`).
  2. `snapshot.append_scaled_texture(texture, filter, bounds)`: filtro **Linear** abaixo de 200% e **Nearest** a partir de 200%, para ver pixel por pixel em inspeção. Configurável.
  3. Sobreposições via `snapshot.append_cairo(...)`, usando o renderizador da seção 6.9.
- **Zoom digital:** 25% a 800%. "Ajustar à janela" é o padrão. A roda do mouse aplica zoom **centrado no cursor**. Arrastar move a imagem (pan) quando nenhuma ferramenta está ativa. Duplo clique alterna entre ajustar e 100%.
- `ViewTransform { scale, offset }` converte entre coordenadas de **imagem** e de **widget**. Essa matemática fica em `hiview-core` e tem testes unitários.
- **Congelar:** a viewport para de aceitar novos frames e guarda o `Arc<Frame>` congelado. Foto e medição usam esse frame.

### 6.6 Foto (`hiview-app::services::photo` + `hiview-core::imaging`)

- Fonte: o frame congelado, se houver, ou então o `latest`.
- **Resolução de salvamento:**
  - Nativa 640×480 (padrão no modo Bancada).
  - Interpolada 1280×960.
  - Interpolada 1600×1200 (equivale ao "2 MP" da caixa).
  - Upscale feito com Lanczos3 (crate `image`). A UI mostra a etiqueta "interpolada".
- **Formato:** PNG (padrão, sem perda) ou JPEG (qualidade 92).
- **Foto com redução de ruído:** média de N quadros consecutivos (4, 8 ou 16), acumulada em `f32` e só então convertida. Exige o microscópio parado. Ela coleta os quadros pelo assinante completo do FrameBus.
- **Gravar sobreposições na foto** (opcional): o renderizador cairo desenha sobre um `cairo::ImageSurface` na resolução de saída.
- Codificação e escrita em thread de trabalho (comando assíncrono do Relm4). **A thread da UI nunca bloqueia.**
- Destino: `glib::user_special_dir(UserDirectory::Pictures)/HiView/`. Respeita o nome localizado da pasta, como "Imagens".
- Nome: `HiView_AAAA-MM-DD_HH-MM-SS.png`. Em colisão, acrescenta `_2`, `_3` e assim por diante.
- Metadados (M8 em diante): chunks `tEXt` no PNG com a calibração ativa e as transformações aplicadas.

### 6.7 Gravação de vídeo (`hiview-capture::recorder`)

```
appsrc name=src is-live=true format=time caps=video/x-raw,format=RGBA,width=W,height=H,framerate=30/1
  ! videoconvert ! video/x-raw,format=I420
  ! x264enc tune=zerolatency speed-preset=veryfast bitrate=2500
  ! h264parse ! mp4mux ! filesink location=<arquivo>.mp4
```

- **Encoder:** `x264enc` (gstreamer1.0-plugins-ugly). Fallback em tempo de execução para `openh264enc`. Se nenhum existir, o botão de gravar fica desabilitado, com explicação.
- **PTS:** o PTS de captura é rebaseado para zero no início da gravação. Não usar `do-timestamp` no appsrc.
- **Parada correta:** `appsrc.end_of_stream()`, depois **esperar `EOS` no bus** (timeout de 5 s), e só então ir para `Null`. ⚠️ Sem isso o MP4 fica sem o átomo `moov` e **não abre**.
- Fechar o app durante uma gravação executa a mesma sequência antes de sair.
- Antes de iniciar, verifica o espaço livre. Abaixo de 500 MB, avisa e não inicia.
- Destino: `glib::user_special_dir(UserDirectory::Videos)/HiView/`, nome `HiView_AAAA-MM-DD_HH-MM-SS.mp4`.
- Indicador de tempo de gravação e contador de quadros descartados (este só no modo Bancada).
- Congelar fica bloqueado durante a gravação.

### 6.8 Galeria

- Lê as duas pastas do HiView e mostra uma grade de miniaturas (`gtk::GridView`), ordenada da mais recente para a mais antiga.
- Miniaturas geradas em thread de trabalho e guardadas em cache em `$XDG_CACHE_HOME/hiview/thumbs/`. Para vídeos, a miniatura sai do primeiro quadro via GStreamer.
- Ações: abrir no visualizador do sistema (`gtk::FileLauncher`), mostrar na pasta, excluir (com confirmação, movendo para a lixeira via `gio::File::trash`).

### 6.9 Sobreposições, medição e calibração

- **Renderizador único:** `OverlayRenderer::draw(cr: &cairo::Context, view: &ViewTransform, state: &OverlayState)`. Todas as geometrias ficam guardadas em **coordenadas de imagem**.
- **Grade:** espaçamento em pixels ou em mm (se houver calibração), cor e opacidade configuráveis.
- **Mira:** central, nos estilos cruz, cruz com círculo e cruz com escala.
- **Medição (modo Bancada):** régua (distância entre 2 pontos), ângulo (3 pontos) e círculo (diâmetro). Ficam visíveis até serem apagadas. Esc cancela a medição em curso.
- **Calibração:**
  1. O usuário fotografa uma régua ou escala conhecida.
  2. Desenha uma linha sobre ela e informa a distância real (em mm).
  3. O app calcula `px_por_mm` e salva um **perfil nomeado** (ex.: "Anel no máximo").
- **Incerteza exibida:** ±1 px convertido para µm. A 640×480 isso é relevante e precisa aparecer.
- **Aviso permanente** no modo Bancada quando há calibração ativa: "Válida apenas se o anel de ampliação e a altura não mudaram desde a calibração".
- Pressupõe pixels quadrados. Isso é verificado no teste de aceitação do M8 (medida horizontal × vertical da mesma régua, diferença < 2%).

### 6.10 Subsistema do botão físico

**`hiview-button`** expõe:

```rust
pub enum ButtonEvent { Pressed, Released }
pub trait ButtonSource: Send { fn start(self: Box<Self>, tx: Sender<ButtonEvent>) -> Result<()>; }
```

A interpretação "clique curto/longo" fica no app, não na fonte:

- Curto (< 700 ms): **foto**.
- Longo (≥ 700 ms): **iniciar/parar gravação**. Configurável.
- Debounce de 50 ms.

**Fontes, em ordem de preferência:**

1. **`EvdevSource`:** procura um input device cujo `phys` esteja no mesmo caminho USB da câmera e que emita `KEY_CAMERA`. Hoje não existe, mas passa a funcionar sozinho se um kernel futuro ou um patch passar a criá-lo.
2. **`HelperSource`:** conecta ao socket `/run/hiview/button.sock` do `hiview-buttond`.
3. **Nenhuma:** o botão físico fica indisponível. A UI mostra, nas preferências, "Botão físico: não disponível — ver manual". O app funciona normalmente.

**`hiview-buttond`** (binário root, serviço systemd):

- Carrega `usbmon` (via `/etc/modules-load.d/hiview.conf`).
- Localiza `05e3:f12a` via udev e obtém `busnum`/`devnum`, acompanhando o hotplug (o `devnum` muda a cada reconexão).
- Lê `/dev/usbmon<busnum>` pela **API binária** (`MON_IOCX_GETX`; ver `Documentation/usb/usbmon.rst` do kernel).
- Filtro: `type == 'C'` && `xfer_type == interrupt` && `epnum == 0x83` && `devnum` confere && `len_cap >= 4` && `data[0] == 0x02` && `data[2] == 0x00`. `data[3]` é 1 (pressionado) ou 0 (solto).
- Publica linhas `PRESSED\n` / `RELEASED\n` no socket Unix `/run/hiview/button.sock` (modo `0660`, grupo `hiview`).
- **Segurança:** o `usbmon` vê **todo** o tráfego do barramento, inclusive teclados. Por isso o helper:
  - nunca repassa nada além desses dois eventos;
  - não tem rede;
  - tem código mínimo;
  - usa hardening no systemd: `ProtectSystem=strict`, `PrivateNetwork=yes`, `NoNewPrivileges=yes`.

  O parser fica em `hiview-core` (puro, testável).
- A instalação do helper é **opcional** (pacote/flag separada) e está documentada no manual.

**Caminho de longo prazo (backlog):** enviar um patch ao `linux-media` adicionando um quirk para `05e3:f12a` no `uvcvideo`. Se for aceito, o `EvdevSource` assume e o helper se torna desnecessário.

### 6.11 Ajustes de imagem (controles V4L2)

- Controles enumerados **dinamicamente** (`VIDIOC_QUERYCTRL`) via crate `v4l`, num fd separado do usado pelo GStreamer (o V4L2 permite isso).
- A UI gera sliders e switches a partir do que o aparelho anuncia (brilho, contraste, saturação, nitidez, gama, balanço de branco etc.). Botão "Restaurar padrões".
- Valores salvos por dispositivo (VID:PID) e reaplicados ao conectar.
- ⚠️ **Pendente:** rodar `v4l2-ctl -d /dev/video2 --list-ctrls` para saber quais controles este aparelho realmente tem.

### 6.12 Configuração e persistência

| O quê | Onde | Formato |
|---|---|---|
| Preferências | `$XDG_CONFIG_HOME/hiview/config.toml` | TOML (serde) |
| Perfis de calibração | mesmo arquivo, seção `[[calibration]]` | TOML |
| Controles V4L2 por dispositivo | mesmo arquivo, seção `[devices."05e3:f12a"]` | TOML |
| Miniaturas | `$XDG_CACHE_HOME/hiview/thumbs/` | PNG |
| Logs | stderr (journal, quando via .desktop) | `tracing` |

Um arquivo de configuração inválido não pode derrubar o app: ele é renomeado para `config.toml.bak` e o app volta aos padrões, mostrando um aviso.

---

## 7. Threads e concorrência

| Thread | Quem | Regras |
|---|---|---|
| Principal (GTK) | Relm4, widgets | **Nunca** bloqueia: sem I/O de disco, sem encode, sem `sleep` |
| Streaming GStreamer | callbacks do appsink | Só copia o frame e publica. Nada de UI aqui |
| Gravador | thread dedicada | Consome o canal do FrameBus e alimenta o appsrc |
| Trabalho | comandos assíncronos do Relm4 / `gio::spawn_blocking` | Encode de foto, miniaturas, empilhamento |
| Botão | thread da fonte | Envia `ButtonEvent` para a UI pelo sender do Relm4 |

---

## 8. Testabilidade sem hardware

O Devin roda na máquina alvo, **com o microscópio conectado**, e pode testar no aparelho real. Mesmo assim, a CI e os testes automatizados **não podem depender de hardware**. Por isso:

- **Fontes alternativas de captura** (flag de CLI `--source`):
  - `--source=device` (padrão): microscópio real.
  - `--source=test`: `videotestsrc pattern=ball`, em 640×480 YUY2 a 30 fps.
  - `--source=file:<caminho>`: reproduz em loop um vídeo de `tests/fixtures/` gravado do microscópio real.
- **Fixtures reais:** no M1, o Devin grava do microscópio 5 s de vídeo e 3 fotos (régua milimetrada, placa SMD, objeto comum; o Henrique posiciona os objetos) em `tests/fixtures/`. Arquivos pequenos: vídeo em MP4, menos de 5 MB no total.
- **Testes unitários obrigatórios** em `hiview-core`:
  - parser do pacote de status (usando os bytes reais da seção 2.1);
  - `ViewTransform`;
  - calibração e incerteza;
  - nomes de arquivo e colisão;
  - média de quadros;
  - carga de config inválida.
- **Teste de integração do gravador:** grava 3 s de `videotestsrc` e valida com `gst-discoverer-1.0` (duração ≈ 3 s, H.264, 640×480).
- **Testes em hardware:** executados pelo Devin no aparelho real. Testes que dependem de ação física (apertar o Snap, girar o anel, posicionar um objeto) são pedidos ao Henrique no relatório. A confirmação final é sempre do Henrique.

---

## 9. Permissões e instalação

- O usuário precisa estar no grupo `video` (padrão no Pop!_OS).
- O pacote `.deb` instala:
  - binário `hiview`;
  - `.desktop`;
  - ícones;
  - metainfo.
- O pacote opcional `hiview-buttond` instala:
  - binário;
  - unit systemd;
  - `modules-load.d`;
  - cria o grupo `hiview` e adiciona o usuário a ele (o postinst informa que é preciso fazer logout/login).
- App ID: `br.com.mundotela.HiView`.

---

## 10. Riscos

| Risco | Prob. | Impacto | Mitigação |
|---|---|---|---|
| Código gtk-rs/Relm4 confuso gerado pelo agente | Média | Médio | `CONVENTIONS.md` rígido, revisão por milestone, componentes pequenos |
| MP4 corrompido ao parar a gravação | Alta se ignorado | Alto | Sequência EOS obrigatória + teste de integração |
| Agente executando comandos na máquina do Henrique | Média | Alto | Proibido `sudo`/`apt` e escrita fora do projeto. Modo de bypass de permissões desligado no editor |
| Uma mudança no `uvcvideo` passa a criar o input device | Baixa | Positivo | `EvdevSource` já previsto, com prioridade sobre o helper |
| `usbmon` ausente ou bloqueado em algum kernel | Baixa | Baixo | Botão físico é opcional. Atalhos de teclado sempre funcionam |
| Expectativa de "2 MP / 1600X" do usuário | Alta | Médio | Rótulos honestos + seção no manual |
| Calibração usada com o anel alterado | Alta | Médio | Aviso permanente + perfis nomeados |

---

## 11. Mapeamento das funções do app Android

| App Android | HiView | Milestone |
|---|---|---|
| Resolution | Resolução de **salvamento** (nativa / interpolada) | M3 |
| Zoom in / Zoom out | Zoom digital da visualização + pan + ajustar/100% | M2 |
| Reference line | Grade e mira configuráveis | M6 |
| Mirror | Espelhar horizontal e vertical | M2 |
| Rotate | Girar ±90° / 180° | M2 |
| Lock | **Congelar quadro** (a trava de orientação não existe no desktop) | M2 |
| Shoot / Shutter | Foto (botão, atalho, botão físico) | M3 / M9 |
| Video | Gravação MP4 | M4 |
| File | Galeria integrada | M5 |
| — | Medição calibrada, ajustes V4L2, redução de ruído, modos Explorar/Bancada | M6–M10 |

---

## 12. Questões em aberto

1. ~~Versões de GTK, libadwaita e GStreamer na máquina do Henrique~~: resolvido (§4).
2. Saída de `v4l2-ctl -d /dev/video2 --list-ctrls`.
3. Os LEDs são controláveis por software? (Depende do item 2.)
4. Licença do projeto (sugestão: GPL-3.0-or-later, compatível com `x264enc`).
5. ~~Repositório~~: resolvido. `github.com/mundotela/HiViewLinuxMicroscopio` (privado). O push é feito pela engenharia.
