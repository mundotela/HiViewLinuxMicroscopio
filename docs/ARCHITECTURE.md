# HiView — Arquitetura

> **Versão:** 1.1 · **Data:** 2026-11-15 · **Status:** HiView 1.1.3 (M0–M12 + correções 1.1.1–1.1.3)
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
| Controles V4L2 | **Nenhum** (`v4l2-ctl --list-ctrls` vazio, confirmado em 2026-11-10) | Brilho/contraste/etc. só por software (§6.11). LEDs só pela roda do cabo |
| Primeiros quadros | O 1º quadro após abrir o dispositivo vem corrompido (buffer não inicializado) | O pipeline descarta os 3 primeiros quadros de fontes de dispositivo antes do FrameBus |
| Decodificador H.264 no sistema | Só `openh264dec`, que lê **apenas constrained-baseline** | Toda gravação usa perfil constrained-baseline (§6.7) |
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
| ADR-003 | **GStreamer** para captura, gravação e sondagem de formatos (`DeviceMonitor`). Ajustes de imagem em Rust (§6.11, decisão do M7). **Sem crate `v4l`** (decisão do M1) | Pipeline maduro, encoder H.264 pronto, MP4 correto. O `v4l` exigiria libclang e não traria nada, já que o aparelho não tem controles |
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
- **Processador (decisão do M7):** **x86-64-v2** ou superior (SSE4.2/POPCNT; Intel a partir de ~2009, AMD a partir de ~2011), via `target-cpu=x86-64-v2` em `.cargo/config.toml`. Necessário para a passada de ajustes de imagem caber no orçamento (§6.11) em Rust seguro: 0,83 ms com v2, contra 1,29 ms com o baseline SSE2. Em processadores mais antigos o app não inicia (instrução ilegal), e esse requisito está no README e no manual. O nível v3/AVX2 (0,59 ms) foi rejeitado por excluir CPUs anteriores a 2013.
- **Toolchain:** Rust stable, edition 2024, **mínimo 1.93** (exigência do `relm4 0.11`). Fonte única: `rust-version` em `[workspace.package]`. O `check-env.sh` lê esse valor de lá.

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
│ ImageAdjust (Rust: LUT + saturação + nitidez)    │        │ /dev/usbmonN → filtro → eventos            │
└──────────────────────────────────────────────────┘        └────────────────────────────────────────────┘
```

---

## 6. Componentes em detalhe

### 6.1 Descoberta de dispositivo (`hiview-capture::device`)

- Enumera o subsistema `video4linux` via crate `udev`. Para cada nó, sobe até o pai `usb_device` e lê `idVendor`/`idProduct`.
- Filtra os nós com capacidade `V4L2_CAP_VIDEO_CAPTURE` no `device_caps`. Isso descarta o `/dev/video3`, que é de metadados.
- **Sondagem de formatos pelo `v4l2deviceprovider` explícito** (decisão do M11): `gst::DeviceProviderFactory::by_name("v4l2deviceprovider")`, casando por `device.path`. **Não** use o `gst::DeviceMonitor` com os providers padrão: quando o `gstreamer1.0-pipewire` está instalado (o padrão no Ubuntu GNOME), o `pipewiredeviceprovider` **esconde** o v4l2, e as câmeras chegam sem `device.path`. No teste de instalação limpa, isso deu "0 câmeras" no Ubuntu 24.04. Se o provider v4l2 faltar, o app mostra uma página de erro dedicada, nunca "0 câmeras" em silêncio. A captura continua por `v4l2src`, que coexiste com o PipeWire.
- **Seleção automática:** prioridade para `05e3:f12a`. Uma tabela `KNOWN_MICROSCOPES` em `hiview-core` permite adicionar outros modelos no futuro.
- **Outras câmeras (M12, v1.1.0):**
  - **Sem o microscópio:** a página "Procurando microscópio…" informa, quando há outra câmera, que ela pode ser usada pelo menu ☰ → Câmera. O menu Câmera aparece com qualquer câmera disponível.
  - **Microscópio conectado depois:** o app troca para ele automaticamente, com aviso, e nunca no meio de uma gravação.
  - **A escolha manual não persiste:** ao reabrir, a prioridade volta ao microscópio.
  - **Formatos:** YUY2 até 1280×720 (≥ 25 fps), senão MJPG + `jpegdec`. Nunca acima de 720p por padrão.
  - **Resolução da foto:** "nativa" é a da câmera ativa; as interpoladas só aparecem se o resultado ficar ≤ 1600×1200.
  - **Lição do M12:** a página inicial só era atualizada por eventos de hotplug. Sem nenhum evento, ela nunca refletia a enumeração real. Agora o estado é aplicado também na inicialização (`post_view`).
- **Hotplug:** `udev::MonitorBuilder` no subsistema `video4linux`. Eventos `add`/`remove` viram `CameraEvent::Connected/Disconnected`.
- O resultado é um `CameraDescriptor { vid, pid, name, dev_node, bus_num, dev_num, usb_path }`. `bus_num`/`dev_num` também são usados pelo helper do botão.

### 6.2 Pipeline de captura (`hiview-capture::pipeline`)

```
v4l2src device=<dev_node> do-timestamp=true
  ! video/x-raw,format=YUY2,width=640,height=480,framerate=30/1
  ! videoconvert
  ! video/x-raw,format=RGBA
  ! videoflip name=flip video-direction=identity
  ! appsink name=sink sync=false max-buffers=4 drop=true
```

- As caps vêm de `CaptureMode`, construído a partir do que o dispositivo anuncia (sondado pelo `gst::DeviceMonitor`). **Nada fica fixo no código além do padrão.**
- Callback `new-sample` (thread de streaming do GStreamer): copia o buffer para um `Frame`, aplica os ajustes de imagem (§6.11, pulado quando todos estão no padrão) e publica no `FrameBus`.
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

- No M1, a viewport é provisoriamente um `gtk::Picture` (só "ajustar à janela"). **A partir do M2**, ela passa a ser o widget próprio descrito abaixo.
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
- **Foto com redução de ruído (implementada no M10):** média de N quadros consecutivos (4, 8 ou 16; padrão desligada), coletados pelo assinante completo do FrameBus (`hiview-app::services::noise`), com prazo de 3 s pelo relógio de chegada. Exige o microscópio parado.
  - **Acumulação inteira** em `hiview-core::average` (`FrameAverager`): soma `u32` por byte e divisão com arredondamento `(soma + N/2) / N`. É bit-exata e mais rápida que `f32`.
  - **A média usa os quadros já ajustados** (§6.11). A LUT é por pixel e o unsharp mask é linear, então a ordem não importa (a menos do clamp em 0/255, que é desprezível). Não existe derivação "crua" no pipeline.
  - **Quadro congelado:** sem média, a foto sai do quadro congelado. Disparos durante uma captura são ignorados. Girar fica bloqueado durante a captura. Desconexão ou prazo estourado cancelam com toast, sem arquivo parcial. Quadros pulados por descarte no canal são aceitos.
  - **Metadado PNG** `noise_reduction_frames` = quantidade de quadros que entraram de fato na média (1 quando desligada ou congelada).
  - **Medido no microscópio** (σ do ruído, referência independente de 64 quadros): 3,35 → 1,84 (N=4) → 1,42 (N=8) → 1,06 (N=16). N=4 e N=8 ficam dentro de ±20% de σ₁/√N. N=16 fica 26% acima, por ruído temporal correlacionado (oscilação de iluminação) que a média não cancela. A coleta leva ~33 ms por quadro (N=16 ≈ 0,53 s). Fixtures MP4 **não servem** para medir ruído (a compressão H.264 remove e correlaciona o ruído do sensor).
- **Gravar sobreposições na foto** (opcional): o renderizador cairo desenha sobre um `cairo::ImageSurface` na resolução de saída.
- Codificação e escrita em thread de trabalho (comando assíncrono do Relm4). **A thread da UI nunca bloqueia.**
- Destino: `glib::user_special_dir(UserDirectory::Pictures)/HiView/`. Respeita o nome localizado da pasta, como "Imagens".
- Nome: `HiView_AAAA-MM-DD_HH-MM-SS.png`. Em colisão, acrescenta `_2`, `_3` e assim por diante.
- Metadados (M8 em diante): chunks `tEXt` no PNG com a calibração ativa e as transformações aplicadas.

### 6.7 Gravação de vídeo (`hiview-capture::recorder`)

```
appsrc name=src is-live=true format=time caps=video/x-raw,format=RGBA,width=W,height=H,framerate=30/1
  ! videoconvert ! video/x-raw,format=I420
  ! x264enc tune=zerolatency speed-preset=superfast threads=1 bitrate=2500
  ! video/x-h264,profile=constrained-baseline
  ! h264parse ! mp4mux ! filesink location=<arquivo>.mp4
```

- **Perfil obrigatório: constrained-baseline.** O único decodificador H.264 do sistema (`openh264dec`) não lê outros perfis. Sem isso, os vídeos não abririam no Totem nem gerariam miniatura na galeria (M5). O teste de integração do M4 deve decodificar o arquivo gravado com `openh264dec`.
- **Encoder:** `x264enc` com `superfast` e `threads=1`, porque `veryfast` estourava o orçamento de CPU (medido no M4: veryfast ~15,6%, superfast ~10,3%, openh264enc ~7,3% de um núcleo; o `openh264enc` fica como opção futura). `x264enc` vem do gstreamer1.0-plugins-ugly. Fallback em tempo de execução para `openh264enc`, que já gera baseline. Se nenhum existir, o botão de gravar fica desabilitado, com explicação.
- **PTS:** o PTS de captura é rebaseado para zero no início da gravação. Não usar `do-timestamp` no appsrc.
- **Índice reservado e atualizado (obrigatório, decisão do M4):** `mp4mux reserved-max-duration=5 min` + `reserved-moov-update-period=1 s`. O índice (`moov`) fica no início do arquivo e é reescrito a cada segundo, então uma gravação interrompida por travamento, `kill -9` ou falta de energia continua reproduzível, com a duração real menos ~1 s. A reserva de 5 min **não limita** a duração: gravações mais longas finalizam normalmente (provado em teste com uma reserva de 10 s e gravação de 20 s, e no aparelho com 6 min 06 s). Reservas maiores foram rejeitadas porque pré-alocam espaço fixo em todo arquivo (2 h ≈ 8 MB, o que inflava clipes curtos para ~5,8 Mbit/s). Motivo da decisão: no teste do M4, um vídeo de 2 min ficou irrecuperável (sem `moov`) depois de um `Ctrl+C`.
- **Desconexão durante a gravação:** erro ou desconexão da câmera finaliza a gravação na hora (EOS), tira o app do estado "gravando" e mostra um aviso. Uma reconexão **nunca** retoma a gravação sozinha.
- **Parada correta:** `appsrc.end_of_stream()`, depois **esperar `EOS` no bus** (timeout de 5 s), e só então ir para `Null`. ⚠️ Sem isso o MP4 fica sem o átomo `moov` e **não abre**.
- Fechar o app durante uma gravação executa a mesma sequência antes de sair. O mesmo vale para **SIGINT (Ctrl+C) e SIGTERM** (logout ou desligamento).
- Antes de iniciar, verifica o espaço livre. Abaixo de 500 MB, avisa e não inicia.
- Destino: `glib::user_special_dir(UserDirectory::Videos)/HiView/`, nome `HiView_AAAA-MM-DD_HH-MM-SS.mp4`.
- Indicador de tempo de gravação e contador de quadros descartados (este só no modo Bancada).
- Congelar fica bloqueado durante a gravação.

**Áudio do microfone (implementado no M10.5, 2026-11-13).** É opcional e fica **desligado por padrão**: Preferências → Vídeo, ou o botão de microfone na barra (`Shift+R`), que é desabilitado durante a gravação.

- **Trilha:** `pulsesrc` (via pipewire-pulse; dispositivo escolhido ou o padrão do sistema) `! audioconvert ! audioresample ! audio/x-raw,rate=48000,channels=1 ! level ! queue ! opusenc bitrate=64000 ! queue ! mp4mux`. É o mesmo mux do vídeo, e o vídeo não muda.
- **Sincronia A/V:** o gravador usa o **mesmo `gst::Clock`** da captura, e o `base_time` é **definido** antes de PLAYING (`set_start_time(NONE)` + `set_base_time(clock.time())`), nunca estimado. O PTS do vídeo é convertido assim: running-time da captura + `base_time` da captura − `base_time` do gravador. A abertura do microfone (~90 ms no hardware) aparece como áudio começando depois, com PTS corretos, sem compensação manual. Uma fonte com 500 ms de atraso de abertura fica dentro de 24 ms de sincronia.
- **Privacidade:** o microfone **só é aberto durante a gravação**. Não há pré-escuta, e nenhum dado de áudio vai para o log.
- **Falhas:**
  - microfone indisponível no início → grava só vídeo, com toast;
  - microfone perdido no meio → EOS nas duas trilhas e finalização limpa, com toast.

  Gravar nunca é bloqueado pelo áudio. O moov reservado continua recuperando arquivos com duas trilhas após `kill -9`.
- **Indicador:** ícone de microfone + `LevelBar` (~8 Hz, alimentada pelo `level`) ao lado do tempo de gravação.
- **Custo:** +2 a +8 pontos de um núcleo enquanto grava (é uma ação, fora do orçamento de regime).

### 6.8 Galeria

> **Implementado no M5.** Pontos de projeto que se mostraram essenciais: **um único** componente de galeria durante toda a vida do app (página empurrada e retirada do `adw::NavigationView`); quadradinhos com **moldura fixa 4:3** e a miniatura contida nela; **clique simples abre**, Enter abre, e botão direito abre um **popover único** pendurado na grade; quadradinhos **não recebem o ponteiro** (`can_target=false`), por causa de um bug do GTK 4.14 com widgets reciclados; o mapeamento clique → item é feito pela grade. A gravação em andamento aparece como um quadradinho "Gravando…" identificado pelo caminho do arquivo (no máximo um por gravação).

- Lê as duas pastas do HiView e mostra uma grade de miniaturas (`gtk::GridView`), ordenada da mais recente para a mais antiga.
- Miniaturas geradas em thread de trabalho e guardadas em cache em `$XDG_CACHE_HOME/hiview/thumbs/`. Para vídeos, a miniatura sai do primeiro quadro via GStreamer.
- Ações: abrir no visualizador do sistema (`gtk::FileLauncher`), mostrar na pasta, excluir (com confirmação, movendo para a lixeira via `gio::File::trash`).

### 6.9 Sobreposições, medição e calibração

> **Implementado no M6 (sobreposições) e no M8 (medição).** Decisões do M8: com uma ferramenta ativa, o clique esquerdo mede e o arraste da imagem passa para o **botão do meio**; atalhos `L` régua, `A` ângulo, `O` círculo; valores abaixo de 1 mm em **µm**; diâmetro com o símbolo **Ø** (o `⌀` não existe nas fontes comuns); rótulos de 19 px em negrito sobre um fundo escuro, em pixels de tela (independentes do zoom e corretos em HiDPI), e proporcionais à resolução de saída na foto; a calibração é medida em **pixels da imagem**; com calibração ativa, a grade e a escala da mira passam a usar **mm**; fotos PNG levam metadados `tEXt` (calibração, transformações, versão). Validado com a lâmina: 1,02 mm ± 0,01 no braço perpendicular à calibração; 993 µm ± 9 no círculo de 1000 µm.

- **Renderizador único:** `OverlayRenderer::draw(cr: &cairo::Context, view: &ViewTransform, state: &OverlayState)`. Todas as geometrias ficam guardadas em **coordenadas de imagem**.
- **Grade:** espaçamento em pixels ou em mm (se houver calibração), cor e opacidade configuráveis.
- **Mira:** central, nos estilos cruz, cruz com círculo e cruz com escala.
- **Medição (modo Bancada):** régua (distância entre 2 pontos), ângulo (3 pontos) e círculo (diâmetro). Ficam visíveis até serem apagadas. Esc cancela a medição em curso.
- **Calibração:**
  1. O usuário coloca sob a lente uma escala conhecida. Referência do projeto: **lâmina de calibração "Microscope Micrometer Calibration Ruler"**, que tem cruz com DIV = 0,1 mm nos dois eixos, grade 0,1 × 0,1 mm, linhas de 0,03/0,05/0,07/0,1/0,2 mm, círculos de 100 a 1000 µm e régua de 40 mm. Uma régua milimetrada comum também serve, com menos precisão.
  2. Desenha uma linha sobre ela e informa a distância real (em mm).
  3. O app calcula `px_por_mm` e salva um **perfil nomeado** (ex.: "Anel no máximo").
- **Incerteza exibida:** ±1 px convertido para µm. A 640×480 isso é relevante e precisa aparecer.
- **Aviso permanente** no modo Bancada quando há calibração ativa: "Calibração válida apenas se o anel de ampliação e a altura não mudaram desde a calibração."
- **Calibração por câmera (M12):** cada perfil pertence a um dispositivo (`CalibrationProfile.device`, chave VID:PID ou caminho USB). A calibração de uma câmera **nunca** é aplicada a outra, porque px/mm depende da lente e da distância. Toda câmera pode ser calibrada; enquanto não for, as medidas saem em px, com o aviso "Sem calibração para esta câmera". Os configs da v1.0 foram migrados: os perfis existentes passam a pertencer ao `05e3:f12a`.
- Pressupõe pixels quadrados. Isso é verificado no teste de aceitação do M8 (medida horizontal × vertical da cruz da lâmina, diferença < 2%).

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

> **Implementado no M9 (2026-11-12).** O que mudou em relação ao plano acima:
>
> - **Hotplug sem udev monitor:** com `PrivateNetwork=yes`, os uevents do kernel (netlink) não chegam ao serviço. O daemon reenumera o sysfs (via `udev::Enumerator`) a cada 1,5 s, no máximo uma vez por tick. O `poll(2)` no fd do usbmon continua acordando na hora para o botão.
> - **Filtro final** (`hiview_core::snap_button_event`): `'C'` + interrupt + `epnum == 0x83` + `busnum`/`devnum` do microscópio + `len_cap == 4` + bytes `02 01 00 0x`.
> - **Truncamento é normal:** o vídeo do próprio microscópio passa pelo mesmo barramento. O kernel copia `min(len_cap, alloc)` bytes e informa o `len_cap` original. Registros maiores que o buffer (256 B) são descartados em silêncio, sem erro.
> - **Ring do usbmon de 1 MiB** (`MON_IOCT_RING_SIZE`) e watchdog de perdas (`MON_IOCG_STATS` a cada 60 s; warn só se `dropped > 0`).
> - **Protocolo do socket:** `PRESSED <ts_us>\n` / `RELEASED <ts_us>\n`. Máximo de 8 clientes. Qualquer erro de escrita derruba o cliente (linha parcial dessincroniza o protocolo).
> - **Relógios:** o `ts_us` do usbmon é **CLOCK_REALTIME** (`ktime_get_real_ts64` em `mon_bin.c`); o do evdev é CLOCK_MONOTONIC. Ele serve **só para deltas da mesma fonte** (duração apertar→soltar). O timer do aperto longo é agendado pelo relógio de **chegada** no app (`timeout_add_local_once`). Nunca comparar `ts_us` com `glib::monotonic_time`.
> - **Status:** Preferências → Botão físico → Status ("Conectado via serviço" / "Conectado via driver" / "Não disponível") + toast **só na mudança de estado**, inclusive na primeira conexão. Tentativas de reconexão são silenciosas (debug).
> - **Unit systemd:** root sem nenhuma capability (`CapabilityBoundingSet=` vazio), `DevicePolicy=closed` + `DeviceAllow=char-usbmon rw`, `RestrictAddressFamilies=AF_UNIX`, `ProtectSystem=strict`, `ProtectHome`, `MemoryDenyWriteExecute`, `SystemCallFilter=@system-service`, `SupplementaryGroups=hiview` (o `chgrp` do socket não precisa de `CAP_CHOWN`). `PrivateDevices` **não** pode ser usado (esconderia `/dev/usbmon*`).
> - **`unsafe`:** só em `hiview-buttond/src/usbmon_ffi.rs`, 3 blocos (GETX, RING_SIZE, STATS), cada um com `// SAFETY:`. Struct de 64 bytes conferida campo a campo contra `usbmon.rst`.
> - **Validação física:** curto = foto, longo = grava/para, replug, `systemctl stop/start` com o app aberto, 0 erros no log. O caminho evdev não foi exercido em hardware real (este microscópio não gera `KEY_CAMERA`).

**Caminho de longo prazo (backlog):** enviar um patch ao `linux-media` adicionando um quirk para `05e3:f12a` no `uvcvideo`. Se for aceito, o `EvdevSource` assume e o helper se torna desnecessário.

### 6.11 Ajustes de imagem (por software)

O microscópio **não expõe nenhum controle V4L2** (§2). Os ajustes são feitos **em Rust**, sobre cada frame RGBA copiado no callback do `appsink`, antes da publicação no `FrameBus`. Por isso aparecem na tela, na foto e no vídeo, que consomem os mesmos pixels.

A implementação mora em `hiview-core::image_adjust` (Rust puro, testável, sem GTK/GStreamer). O `CapturePipeline::set_adjust` compila um `AdjustPlan` e o troca atomicamente no estado compartilhado com o callback: ajuste **ao vivo**, sem reconstruir o pipeline. Com todos os valores no padrão, o plano é `None` e o frame passa **bit-exato**, com custo zero.

> **Decisão do M7 (2026-11-11), substituindo o desenho original com elementos GStreamer:** o `gaussianblur` mínimo custava ~30% de um núcleo sozinho (todos os ajustes: 38,9%; gravando: 51,1%), e `videobalance` + `gamma` sem nitidez custavam ~10 pontos. A alternativa `vapostproc` (GPU, ~2%) foi rejeitada porque depende de VA-API, indisponível em placas NVIDIA. Detalhes em `docs/reports/M7.md`.

| Ajuste | Implementação | Faixa na UI |
|---|---|---|
| Brilho | LUT de 256 entradas | −1,0 … +1,0 (padrão 0) |
| Contraste | mesma LUT (pivô no cinza médio) | 0 … 2,0 (padrão 1) |
| Gama | mesma LUT (`x^(1/γ)`; γ>1 levanta os médios) | 0,3 … 3,0 (padrão 1) |
| Saturação | mistura com a luma BT.601, aritmética inteira | 0 … 2,0 (padrão 1) |
| Nitidez | unsharp mask 3×3 **na luma** (o delta é somado a R, G e B, sem franjas coloridas), aritmética inteira, bordas replicadas | 0 … 100% contínuo (padrão 0) |

- Brilho + contraste + gama fundem-se em **uma LUT**, recalculada só quando um valor muda. A nitidez usa um anel de três linhas (estágio 1 computado na hora), num **único passe** por frame, sem cópia do quadro inteiro.
- Cada estágio só roda quando o valor sai do padrão.
- O passe usa **apenas Rust seguro**: laços planos sobre fatias (`chunks_exact`/`zip`) que o compilador auto-vetoriza, sem `unsafe`, sem intrínsecos SIMD e sem threads extras (o orçamento do §7 conta o tempo de *toda* thread).
- **Codegen:** `.cargo/config.toml` fixa `target-cpu=x86-64-v2` (§4). O orçamento só fecha quando o compilador pode usar lanes de 16 bits (`pmullw`/`pmovzx`). Medido no M7: 1,29 ms → 0,83–0,93 ms por frame. Escopo restrito a `x86_64`, sem efeito em outras arquiteturas.
- A UI agrega mudanças de slider (~80 ms).
- Valores salvos por dispositivo (chave = VID:PID) e reaplicados ao conectar. Botão "Restaurar padrões".
- Futuro: se um aparelho expuser controles V4L2 reais (ou o quirk do `uvcvideo` for aceito), eles podem ser adicionados ao lado. Isso fica fora do escopo da v1.

### 6.12 Configuração e persistência

| O quê | Onde | Formato |
|---|---|---|
| Preferências | `$XDG_CONFIG_HOME/hiview/config.toml` | TOML (serde) |
| Perfis de calibração | mesmo arquivo, seção `[[calibration]]` | TOML |
| Controles V4L2 por dispositivo | mesmo arquivo, seção `[devices."05e3:f12a"]` | TOML |
| Miniaturas | `$XDG_CACHE_HOME/hiview/thumbs/` | PNG |
| Logs | stderr (journal, quando via .desktop) | `tracing` |

Um arquivo de configuração inválido não pode derrubar o app: ele é renomeado para `config.toml.bak` e o app volta aos padrões, mostrando um aviso.

**Gravação atômica (1.1.1):** a configuração é gravada num arquivo temporário, com `fsync` e depois `rename`, por **um único** gravador que agrupa os pedidos e grava sempre a versão mais recente. Antes, cada mudança criava uma thread e usava `fs::write` (que trunca e reescreve), e um encerramento no meio deixou o `config.toml` do Henrique com **0 bytes**.

---

## 7. Threads e concorrência

| Thread | Quem | Regras |
|---|---|---|
| Principal (GTK) | Relm4, widgets | **Nunca** bloqueia: sem I/O de disco, sem encode, sem `sleep` |
| Streaming GStreamer | callbacks do appsink | Copia o frame, aplica o `AdjustPlan` (§6.11) e publica. Nada de UI aqui |
| Gravador | thread dedicada | Consome o canal do FrameBus e alimenta o appsrc |
| Trabalho | comandos assíncronos do Relm4 / `gio::spawn_blocking` | Encode de foto, miniaturas, empilhamento |
| Botão | thread da fonte | Envia `ButtonEvent` para a UI pelo sender do Relm4 |

**Orçamento de CPU (revisado no M7):** "% de um núcleo" varia muito com o governador de frequência do processador (no M7, o mesmo binário do M6 mediu 8,5% numa sessão e 18,8% noutra). Por isso o orçamento usa réguas que não dependem disso:
- **Relativo, na mesma sessão:** toda medição compara com a linha de base (`main` atual, vídeo ao vivo) **medida na mesma sessão**, em build release, Wayland nativo e sem ferramentas de injeção rodando. Uma funcionalidade nova em regime contínuo pode acrescentar no máximo o que o seu milestone definir (ex.: ajustes de imagem, todos ativos: ≤ +6 pontos).
- **Tempo por quadro:** trabalho feito em todo quadro (ex.: ajustes de imagem) tem orçamento em ms, medido em benchmark: mediana ≤ 1,0 ms e p95 ≤ 2,0 ms por quadro de 640×480.
- **Referência absoluta (informativa):** em condições normais, o vídeo ao vivo fica em ~8–9% de um núcleo (M3–M6).
- **Ações não entram no orçamento de regime** (gravar, rajada de fotos, gerar miniaturas). Para elas, o critério é: vídeo ≥ 29 fps, zero quadros descartados e UI sem travar. Gravando, o `x264enc` sozinho custa ~10–23 pontos, conforme a frequência do processador.
- O `videoflip` custa ~4–5 pontos e foi aceito (M2).
- Em regime contínuo, com os ajustes no padrão, o app inteiro segue abaixo de 25% de um núcleo.
- Todo milestone que mexe no pipeline mede e reporta a CPU **e o perfil de energia usado** (economia, equilibrado ou desempenho).

**Filas de trabalho limitadas:** toda fila que guarda quadros (fotos, gravação, empilhamento) tem capacidade máxima. Quando cheia, o disparo é recusado com aviso ao usuário, e nunca acumula memória sem limite. Fotos: no máximo 16 pendentes.

---

## 8. Testabilidade sem hardware

O Devin roda na máquina alvo, **com o microscópio conectado**, e pode testar no aparelho real. Mesmo assim, a CI e os testes automatizados **não podem depender de hardware**. Por isso:

- **Fontes alternativas de captura** (flag de CLI `--source`):
  - `--source=device` (padrão): microscópio real.
  - `--source=test`: `videotestsrc pattern=ball`, em 640×480 YUY2 a 30 fps.
  - `--source=file:<caminho>`: reproduz em loop um vídeo de `tests/fixtures/` gravado do microscópio real.
- **Fixtures reais:** no M1, o Devin grava do microscópio 5 s de vídeo e 3 fotos (lâmina de calibração enquadrando a cruz DIV = 0,1 mm, placa SMD, objeto comum; o Henrique posiciona os objetos) em `tests/fixtures/`. Arquivos pequenos: vídeo em MP4, menos de 5 MB no total.
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
- **Pacotes (M11, cargo-deb, lintian sem erros e bloqueante no CI):**
  - `hiview`: `/usr/bin/hiview`, `.desktop`, ícones (SVG + simbólico), metainfo, man page `hiview(1)`, manual em `/usr/share/doc/hiview/USER_MANUAL.md`.
  - `hiview-buttond` (opcional): `/usr/libexec/hiview-buttond`, unit em `/usr/lib/systemd/system/` (hardening idêntico ao revisado no M9), `/usr/lib/modules-load.d/hiview.conf` e `/usr/lib/sysusers.d/hiview.conf`. O postinst cria o grupo (`systemd-sysusers`), ativa o serviço e adiciona o `SUDO_USER` ao grupo. O purge remove só o grupo. Os scripts de mantenedor foram revisados linha a linha antes do primeiro `sudo`.
  - Mantenedor: `MundoTela Tecnologia <contato@mundotela.com.br>`.
  - `hiview-dbgsym` (`.ddeb`, desde a 1.1.2): os símbolos de depuração separados (`debug = "line-tables-only"` + `separate-debug-symbols`). O `.deb` do app continua sem símbolos (~1,4 MB). Com o `.ddeb` instalado, `sudo gdb -p <PID> -batch -ex "bt 60"` mostra os nomes das funções e as linhas do HiView. Foi assim que o laço da 1.1.2 foi localizado.
- **Instalação a partir do fonte** (alternativa ao `.deb`): o serviço do botão é instalado por `scripts/install-buttond.sh` (idempotente, com `--uninstall`). O script **não compila**: o usuário roda `cargo build --release -p hiview-buttond` antes, sem sudo, para não deixar arquivos de root em `target/`. O script recusa rodar sem `sudo` ou num shell root direto e instala em `/usr/local/libexec`, `/etc/systemd/system` e `/etc/modules-load.d`.

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
2. ~~Controles V4L2~~: resolvido. **Nenhum** (§2, §6.11).
3. ~~LEDs controláveis?~~: resolvido. **Não**, só pela roda do cabo.
4. ~~Licença~~: resolvido (2026-11-10). **GPL-3.0-or-later**, copyright MundoTela Tecnologia (`LICENSE` na raiz com o texto oficial da GPLv3, `license = "GPL-3.0-or-later"` no workspace, `publish = false` em todos os crates).
5. ~~Repositório~~: resolvido. `github.com/mundotela/HiViewLinuxMicroscopio` (privado). O push é feito pela engenharia.
