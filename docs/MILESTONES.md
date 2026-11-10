# HiView — Milestones

> **Versão:** 0.1 · **Data:** 2026-11-10
> Referências: `ARCHITECTURE.md` (o quê e por quê), `CONVENTIONS.md` (como)

---

## Fluxo de trabalho

O Devin roda como **Devin Local**, no editor do Devin, **na própria máquina alvo** (Pop!_OS do Henrique), com o microscópio conectado.

```
Engenharia (Henrique + Claude)          Devin Local
        │  prompt do milestone  ─────────►  │
        │                                   │ branch local m<N>-<slug>
        │                                   │ implementa + testes (inclusive no microscópio real)
        │                                   │ relatório em docs/reports/M<N>.md + no chat
        │  ◄─────────────────── relatório   │
        │ revisão (git diff main...m<N>-<slug>) + teste no hardware
        ├── aprovado → engenharia faz merge em main (+ push quando quiser) → próximo prompt
        └── ajustes  → prompt de correção (mesmo branch)
```

**Regras:**

- Um milestone = um branch local = um relatório.
- O Devin **não faz push, não abre PR e não faz merge**. Isso é papel da engenharia.
- O Devin **nunca roda `sudo`/`apt`** nem altera arquivos fora da pasta do projeto. Se faltar um pacote do sistema, ele para e pede.
- O Devin **não começa** o milestone seguinte sem aprovação explícita.
- Se algo na arquitetura parecer errado ou impossível, o Devin **para e pergunta** no relatório, em vez de improvisar.
- Itens "Fora de escopo" não devem ser implementados, mesmo que pareçam fáceis.

### Definition of Done (vale para todo milestone)

- [ ] `cargo fmt --check` passa
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passa
- [ ] `cargo test --workspace` passa
- [ ] CI verde (verificado após o push. Até lá, o workflow precisa existir e os mesmos comandos precisam passar localmente)
- [ ] Nenhum `unwrap()`/`expect()` fora de testes sem comentário `// INVARIANT:` justificando
- [ ] `USER_MANUAL.md` atualizado nas seções afetadas
- [ ] Relatório preenchido conforme o template
- [ ] Funciona com `--source=test`, e com `--source=file:` quando houver fixture

---

## M0 — Fundação

**Objetivo:** projeto compilando, CI rodando e janela vazia com a cara final.

**Escopo:**

- Workspace com os 5 crates de `ARCHITECTURE.md §5` (com conteúdo mínimo onde ainda não há código).
- `hiview-app`: janela `adw::ApplicationWindow` com `adw::HeaderBar`, título "HiView", área central com `adw::StatusPage` "Procurando microscópio…".
- App ID `br.com.mundotela.HiView`. Logging com `tracing` (`RUST_LOG`).
- CLI com `clap`: `--source=device|test|file:<path>` (apenas aceita e registra no log por enquanto).
- CI (GitHub Actions, `ubuntu-24.04`): fmt, clippy, test.
- `docs/` com os 5 documentos. `README.md` com instruções de build e dependências do sistema.
- Script `scripts/check-env.sh`: imprime as versões de rustc, GTK, libadwaita e GStreamer e falha se estiverem abaixo do mínimo.

**Fora de escopo:** qualquer captura de vídeo.

**Critérios de aceite:**

1. `cargo run -p hiview-app` abre a janela no Pop!_OS do Henrique.
2. `scripts/check-env.sh` passa. O relatório traz a saída completa.
3. Workflow de CI criado em `.github/workflows/`. Ele será validado no primeiro push.

**Teste no hardware (Henrique):** rodar o `check-env.sh` e abrir o app.

---

## M1 — Descoberta e vídeo ao vivo

**Objetivo:** ver a imagem do microscópio na tela.

**Escopo:**

- `hiview-capture::device`: enumeração udev, filtro `VIDEO_CAPTURE`, prioridade para `05e3:f12a`, hotplug.
- `CapturePipeline` (§6.2) + `FrameBus` (§6.3) + widget `Viewport` (§6.5), por enquanto só com "ajustar à janela".
- Máquina de estados da câmera, com telas para Procurando / Desconectado / Erro (`adw::StatusPage`).
- Seletor de câmera no menu (quando houver mais de uma).
- Fontes `--source=test` e `--source=file:`.
- Indicador de FPS real (apenas em build debug ou com `--debug-overlay`).

**Fora de escopo:** zoom, transformações, foto.

**Critérios de aceite:**

1. Com o microscópio e a webcam Acer conectados, o app abre o **microscópio** automaticamente.
2. Desconectar o microscópio leva à tela "Desconectado" sem travar. Reconectar volta ao vídeo sozinho em até 3 s.
3. A taxa estável fica ≥ 29 fps no indicador. O uso de CPU fica < 15% de um núcleo na máquina do Henrique.
4. O teste de integração com `--source=test` roda no CI (pipeline sobe e chegam ≥ 30 frames em 2 s).

**Teste no hardware:** o Devin testa os critérios 1–3 no microscópio real. O Henrique confirma visualmente.
**Fixtures:** o Devin grava `tests/fixtures/` a partir do microscópio (ver `ARCHITECTURE.md §8`). O Henrique posiciona os objetos (régua milimetrada, placa SMD, objeto comum) quando o Devin pedir.

---

## M2 — Visualização: zoom, transformações e congelar

**Escopo:**

- `ViewTransform` em `hiview-core`, com testes.
- Zoom de 25–800%: roda do mouse centrada no cursor, `+`/`-`, `0` (ajustar), `1` (100%), duplo clique alterna.
- Pan arrastando a imagem. Filtro Nearest a partir de 200%.
- Espelhar H/V e girar ±90° (via `videoflip`).
- Congelar quadro (`F`), com indicador visual claro ("CONGELADO" + borda).
- Tela cheia (`F11`, `Esc` sai).
- Barra de ferramentas inferior flutuante no estilo do app Android (zoom, espelhar, girar, congelar).

**Critérios de aceite:**

1. O zoom mantém fixo o ponto sob o cursor (teste unitário + teste manual).
2. Girar 90° mostra a imagem em 480×640 sem distorção.
3. Congelado, a imagem não muda mesmo movendo o objeto. Ao descongelar, volta ao vivo.

---

## M3 — Foto

**Escopo:**

- Botão disparador grande (central, como no app Android) + `Espaço`.
- Resolução de salvamento: nativa / 1280×960 / 1600×1200, com rótulo "interpolada" nas duas últimas.
- PNG/JPEG. Pasta e nomes conforme §6.6. Encode fora da thread de UI.
- `adw::Toast` "Foto salva", com ação "Abrir".
- Efeito visual de flash curto ao disparar.
- Preferências (`adw::PreferencesDialog`): formato, resolução e pasta.
- Persistência em `config.toml`.

**Critérios de aceite:**

1. A foto salva corresponde ao que está na tela (espelhado/girado igual; zoom **não** afeta).
2. Com o quadro congelado, salva o quadro congelado.
3. 10 disparos rápidos geram 10 arquivos distintos, sem travar a UI.
4. Config inválida não derruba o app (teste unitário).

---

## M4 — Vídeo

**Escopo:**

- Recorder (§6.7) com a sequência EOS obrigatória.
- Alternância Foto/Vídeo como no app Android + `R` para gravar/parar.
- Indicador vermelho com tempo decorrido.
- Bloqueio de girar e congelar durante a gravação.
- Verificação de espaço em disco. Fallback de encoder.
- Fechar o app gravando finaliza o arquivo corretamente.

**Critérios de aceite:**

1. Teste de integração: 3 s de `videotestsrc` geram um MP4 válido (`gst-discoverer-1.0`, duração 3 s ± 0,2).
2. Um vídeo de 60 s no hardware abre no Totem/VLC e tem duração correta.
3. Fechar a janela no meio da gravação gera um arquivo válido.

---

## M5 — Galeria

**Escopo:** galeria (§6.8) em painel/página própria, com miniaturas em cache, abrir, mostrar na pasta e mover para a lixeira com confirmação. `Ctrl+G` abre.

**Critérios de aceite:**

1. 200 arquivos na pasta: a galeria abre em < 1 s (miniaturas carregam progressivamente).
2. Excluir manda para a lixeira (recuperável).

---

## M6 — Modos Explorar/Bancada + grade e mira

**Escopo:**

- Seletor de modo na HeaderBar (`adw::ToggleGroup` ou `adw::ViewSwitcher`) + `M`.
- Explorar: interface limpa.
- Bancada: painel lateral (`adw::OverlaySplitView`) com as ferramentas.
- `OverlayRenderer` (§6.9) com grade (`G`) e mira (`C`) configuráveis.
- Opção "Gravar sobreposições na foto".

**Critérios de aceite:**

1. A grade e a mira acompanham zoom/pan corretamente.
2. A foto com "gravar sobreposições" ligado mostra exatamente a mesma grade da tela, na escala da resolução de saída.

---

## M7 — Ajustes de imagem (V4L2)

**Pré-requisito:** o Henrique envia a saída de `v4l2-ctl --list-ctrls`.

**Escopo:** controles dinâmicos (§6.11) no painel Bancada (e um subconjunto simples — brilho e contraste — no Explorar), com restaurar padrões e persistência por dispositivo.

**Critérios de aceite:**

1. Todos os controles listados pelo `v4l2-ctl` aparecem e funcionam.
2. Os valores são reaplicados ao reconectar.

---

## M8 — Medição e calibração

**Escopo:**

- Ferramentas de régua (`L`), ângulo e círculo.
- Assistente de calibração.
- Perfis nomeados.
- Incerteza exibida.
- Aviso permanente sobre o anel de ampliação.
- Metadados `tEXt` no PNG.

**Critérios de aceite:**

1. Com uma régua milimetrada: calibrar em 1 mm e medir 2 mm resulta em 2,00 mm ± a incerteza mostrada.
2. Medida horizontal × vertical da mesma régua: diferença < 2%.
3. Medidas funcionam também com o quadro congelado e com zoom.

---

## M9 — Botão físico Snap

**Escopo:**

- Parser em `hiview-core` (testado com os bytes reais: `02010001`, `02010000`).
- `hiview-button` (`EvdevSource` + `HelperSource`).
- `hiview-buttond` com a API binária do usbmon, hotplug e hardening systemd.
- Clique curto = foto. Clique longo = gravar/parar (configurável).
- Status do botão nas preferências.

**Critérios de aceite:**

1. Com o helper instalado, o clique curto tira foto e o longo grava/para.
2. Desconectar e reconectar o microscópio mantém o botão funcionando.
3. Sem o helper, o app funciona normalmente e informa "não disponível".
4. Revisão de segurança: o helper não expõe nenhum dado além de PRESSED/RELEASED (revisão de código pela engenharia).

---

## M10 — Foto com redução de ruído

**Escopo:** opção "Redução de ruído" (média de 4/8/16 quadros), com barra de progresso curta e aviso "mantenha parado".

**Critérios de aceite:** em foto de placa SMD, o ruído visivelmente menor na comparação lado a lado (avaliação do Henrique). Teste unitário da média.

---

## M11 — Empacotamento e acabamento

**Escopo:**

- `.deb` (`cargo-deb`) para `hiview` e `hiview-buttond`.
- Ícone, `.desktop`, metainfo.
- Janela de atalhos (`Ctrl+?`).
- Diálogo "Sobre".
- Revisão final do manual.
- Primeira execução com mini tour de 3 telas (opcional, `adw::Carousel`).

**Critérios de aceite:** instalação limpa num Pop!_OS recém-instalado funciona seguindo **apenas** o manual.

---

## Backlog (sem milestone)

- Patch upstream do quirk `05e3:f12a` no `uvcvideo`.
- Flatpak.
- Timelapse (modo 5 fps).
- Anotações de texto e setas na foto.
- Comparação lado a lado (referência × ao vivo) para inspeção de placas.
- Gravação com sobreposições.
- Internacionalização (gettext) e inglês.
- Suporte a outros microscópios (`KNOWN_MICROSCOPES`).
