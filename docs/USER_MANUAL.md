# HiView — Manual do Usuário

> **Versão 1.0.0** — descreve o aplicativo HiView tal como ele está instalado.

---

## 1. O que é o HiView

O HiView transforma seu microscópio USB numa ferramenta de verdade no computador. Com ele você pode:

- **ver** a imagem ampliada em tela cheia;
- **fotografar** e **gravar vídeos**;
- **medir** componentes, trilhas e soldas com precisão (depois de calibrar);
- organizar tudo numa **galeria**.

Há dois jeitos de usar:

- 🔍 **Modo Explorar:** para quem quer apenas olhar as coisas de perto (um inseto, uma folha, uma nota de dinheiro). Simples e direto.
- 🔧 **Modo Bancada:** para quem trabalha com eletrônica. Acrescenta grade, mira, régua, ajustes de imagem e congelar quadro.

Você pode trocar de modo a qualquer momento, no topo da janela ou com a tecla `M`.

---

## 2. Conheça seu microscópio

| Parte | Para que serve |
|---|---|
| **Anel de ampliação** (marcado 50X ◀▶ 1600X) | Gira para focar e ampliar. Ajuste até a imagem ficar nítida |
| **Botão Snap** | Tira foto (clique curto) ou inicia/para a gravação (clique longo). Exige o componente opcional, veja a seção 10 |
| **Roda no cabo** | Controla o brilho dos LEDs |
| **Suporte** | Ajuste a altura para enquadrar. Quanto mais perto do objeto, maior a ampliação |

### Sobre "2 MP" e "1600X" — leia isto

A caixa promete 2 megapixels e 1600 vezes de aumento. Na prática:

- O sensor captura **640×480 pixels** (cerca de 0,3 MP). Valores maiores são obtidos **esticando** a imagem por software (interpolação), sem detalhe novo.
- O "1600X" depende do tamanho da tela onde você vê a imagem. Não é um aumento óptico real.

O HiView é honesto sobre isso: quando você escolhe salvar em resolução maior, a opção aparece como **"interpolada"**. A boa notícia: 640×480 é suficiente para inspecionar soldas, ler códigos de CIs e ver detalhes que o olho não alcança.

---

## 3. Instalação

**Requisitos:** Pop!_OS ou Ubuntu 24.04 (ou superior) e processador
x86-64 com SSE4.2 (Intel a partir de ~2009, AMD a partir de ~2011 —
praticamente qualquer computador atual). Se o HiView não abrir em um
computador muito antigo, esse é o motivo mais provável.

### 3.1 Programa principal

Baixe o arquivo `hiview_1.0.0-1_amd64.deb` e instale:

```bash
sudo apt install ./hiview_1.0.0-1_amd64.deb
```

Depois, procure **HiView** no menu de aplicativos. O ícone é um
microscópio verde-água.

> Uma cópia deste manual fica instalada em
> `/usr/share/doc/hiview/USER_MANUAL.md`, e a página de manual
> `man hiview` resume as opções de linha de comando.

**Para desinstalar:**

```bash
sudo apt remove hiview
```

### 3.2 Botão físico Snap (opcional)

O sistema Linux não reconhece o botão Snap deste microscópio sozinho.
Um pequeno serviço opcional, o `hiview-buttond`, resolve isso:

```bash
sudo apt install ./hiview-buttond_1.0.0-1_amd64.deb
```

**Depois de instalar, saia da sessão e entre novamente**
(logout/login). Isso é necessário para liberar o acesso ao botão.
O pacote já cria o grupo `hiview` e adiciona o seu usuário a ele.
Se você instalou de um terminal sem sessão gráfica e a mensagem pedir,
rode `sudo usermod -aG hiview <seu-usuário>` e faça logout/login.

**Para desinstalar:**

```bash
sudo apt remove hiview-buttond
```

> **Sobre segurança:** esse componente roda com privilégios de
> administrador porque só o root pode observar a comunicação USB
> (`/dev/usbmon`). Ele foi feito para enviar ao HiView **somente** duas
> mensagens — "botão apertado" e "botão solto". Nenhum dado do USB
> (imagem, vídeo ou qualquer outro tráfego) sai do serviço, ele não
> grava arquivos e não tem acesso à rede. Se preferir não instalar,
> tudo funciona normalmente pelo teclado e pelo mouse.

### 3.3 Migração: instalado antes pelo script

Se o botão já foi instalado pelo script `install-buttond.sh`
(versões anteriores ao pacote), desinstale pelo script **antes** de
instalar o `.deb`, para não deixar dois serviços disputando o mesmo
nome:

```bash
sudo ./scripts/install-buttond.sh --uninstall
sudo apt install ./hiview-buttond_1.0.0-1_amd64.deb
```

O script continua no repositório para quem compila da fonte, mas o
`.deb` é o caminho recomendado.

---

## 4. Primeiro uso

1. Conecte o microscópio a uma porta USB.
2. Abra o HiView. Ele encontra o microscópio sozinho, mesmo que o notebook tenha webcam.
3. Na **primeira vez**, um pequeno tour de boas-vindas em três telas
   apresenta o programa (pode pular com **Pular**). Ele aparece só
   essa vez — para revê-lo, abra o menu ☰ → **Tour de boas-vindas**.
4. Coloque o objeto sob a lente e gire o **anel de ampliação** até focar.
5. Ajuste o brilho na **roda do cabo**.

Se aparecer **"Procurando microscópio…"**, veja a seção 12.

> O menu ☰ (canto superior direito) reúne: **Preferências**,
> **Atalhos de teclado** (a mesma lista da seção 13, também aberta
> com `Ctrl+?`), **Tour de boas-vindas**, **Sobre o HiView** e
> **Sair**.

---

## 5. A tela principal

```
┌──────────────────────────────────────────────────────────┐
│ [Explorar | Bancada]        HiView        [Galeria] [☰]  │  ← barra superior
├──────────────────────────────────────────────────────────┤
│                                                          │
│                 imagem do microscópio                    │
│                                                          │
│        ┌──────────────────────────────────────┐          │
│        │ ⊕ ⊖ ⟲ ⟳ ⇋ ❄       ( ● )      Foto|Vídeo │          │  ← barra flutuante
│        └──────────────────────────────────────┘          │
└──────────────────────────────────────────────────────────┘
```

O HiView tem dois modos, escolhidos no seletor do topo ou pela tecla `M`:

- **Explorar** é o modo simples: a imagem ocupa tudo e não há painel
  lateral nem sobreposições. É o modo padrão para uso casual.
- **Bancada** é o modo de inspeção: abre um **painel lateral** com
  grade, mira, ajustes de imagem e medição. As
  sobreposições ligadas na Bancada voltam a aparecer quando você
  retorna ao modo — mas só aparecem na Bancada.
- No **Explorar**, os ajustes de brilho e contraste ficam no botão ☀
  da barra flutuante — os demais só aparecem na Bancada.

No modo Explorar, as teclas `G` e `C` mostram um aviso rápido de que
essas ferramentas ficam no modo Bancada.

---

## 6. Ver melhor

| Função | Como fazer |
|---|---|
| **Aproximar/afastar (zoom)** | Roda do mouse (aproxima onde está o cursor), botões ⊕ ⊖ ou teclas `+` `-` |
| **Mover a imagem** | Clique e arraste |
| **Ajustar à janela** | Tecla `0` |
| **Tamanho real (100%)** | Tecla `1` |
| **Alternar ajuste/100%** | Duplo clique — alterna entre ajustar à janela e 100% |
| **Espelhar** | `H` (horizontal) / `V` (vertical) |
| **Girar** | `[` e `]` (90° para cada lado) |
| **Tela cheia** | `F11` (sair: `Esc`) |
| **Congelar a imagem** | `F` ou botão ❄. A imagem para (a tela mostra **CONGELADO** com uma borda âmbar) e a câmera continua filmando por baixo. Aperte de novo para voltar ao vivo |

> O zoom do programa é **digital**: ele aumenta a imagem na tela, mas não cria detalhe novo. Para ampliar de verdade, use o anel do microscópio ou aproxime o aparelho do objeto.
>
> A partir de 200% de zoom, a imagem passa a mostrar os pixels "quadradinhos", sem suavização. Isso é proposital, para você ver exatamente o que o sensor captou.

---

## 7. Fotos

- Clique no **botão redondo grande** ou aperte `Espaço`.
- A foto vai para a pasta **Imagens/HiView**.
- Uma mensagem "Foto salva" aparece com o botão **Abrir**.

O que entra na foto:

- ✅ Espelhar e girar entram na foto (ela sai igual à tela).
- ❌ O zoom **não** entra: a foto sempre tem o quadro inteiro.
- ✅ Com a imagem congelada, a foto salva o quadro congelado.

**Opções** (menu ☰ → Preferências → Fotos):

| Opção | Escolhas |
|---|---|
| Formato | PNG (melhor qualidade, padrão) ou JPEG (arquivo menor) |
| Resolução | Nativa 640×480 · 1280×960 *(interpolada)* · 1600×1200 *(interpolada)* |
| Pasta | Pasta onde as fotos são salvas (padrão: **Imagens/HiView**) |
| Redução de ruído | Desligada · 4 · 8 · 16 quadros. Combina várias capturas numa foto mais limpa. **Mantenha o microscópio e o objeto parados** durante a captura (menos de 1 segundo) |
| Gravar sobreposições | Inclui grade, mira e medições na foto, iguais à tela |

> Fotos **PNG** guardam também metadados de texto com a calibração
> ativa (nome e px/mm), o espelhamento, a rotação, a versão do HiView
> e quantos quadros foram combinados (`noise_reduction_frames`) —
> útil para conferir depois como a foto foi tirada. Fotos JPEG não
> levam metadados.

### 7.1 Redução de ruído

Em pouca luz, a imagem sai com "sujinho" (pontos coloridos que tremem).
A redução de ruído junta várias capturas seguidas numa foto só — o
ruído, que muda a cada quadro, se cancela; o objeto, que fica parado,
permanece nítido. O resultado é uma foto bem mais limpa.

- **Quando usar:** pouca luz, ISO alto aparente, objeto **parado**
  (placa apoiada na mesa, lâmina, componente em repouso).
- **Quando não usar:** qualquer coisa que se mexe — incluindo a sua
  mão segurando o microscópio. Se algo se mover durante a captura,
  a foto sai **borrada** (efeito fantasma).
- Enquanto captura, aparece o aviso **"Mantenha parado…"** com uma
  barra de progresso; a foto sai quando a barra fecha (menos de
  1 segundo na opção de 16 quadros).
- Com a **imagem congelada** não há quadros novos para combinar — a
  foto sai do quadro congelado, sem redução de ruído (os metadados
  registram `noise_reduction_frames=1`).
- Se a captura não puder terminar (câmera desligada, imagem mudou de
  tamanho, tempo esgotado), o HiView avisa **"Foto cancelada: …"** e
  nenhum arquivo incompleto é gravado.
- Funciona junto com a **gravação de vídeo**: gravar e fotografar com
  redução ao mesmo tempo é permitido.
- O botão girar fica bloqueado durante a captura — girar mudaria o
  tamanho dos quadros e estragaria a média.

---

## 8. Vídeos

- Mude para **Vídeo** na barra flutuante e clique no botão redondo, ou aperte `R` a qualquer momento.
- Um indicador vermelho mostra o tempo de gravação.
- Para parar, clique de novo ou aperte `R`.
- Os vídeos vão para **Vídeos/HiView**, em MP4 (abrem em qualquer player e celular).
- Durante a gravação, **girar** e **congelar** ficam desativados.
- Cada minuto ocupa cerca de 15–20 MB. O HiView avisa se o disco estiver quase cheio.

### 8.1 Gravar com som (opcional)

Por padrão os vídeos saem **sem som**. Para gravar também a voz (útil para narrar o que está sendo observado):

- Ligue em **Preferências → Vídeo → Gravar áudio do microfone**, ou clique no botão de **microfone** na barra flutuante, ou aperte `Shift+R`. Os três fazem a mesma coisa.
- Em **Microfone** você escolhe qual entrada usar; **Padrão do sistema** atende a maioria dos casos.
- Gravando com som, aparecem no indicador vermelho um **ícone de microfone** e um **medidor** que se mexe conforme você fala — assim dá para ver que o som está entrando.
- O botão de microfone fica **desativado durante a gravação**: não dá para ligar ou desligar o som no meio de um vídeo.
- Vale para qualquer forma de iniciar a gravação: botão, `R` e o clique longo do botão Snap.

**Privacidade:** o microfone só é aberto durante a gravação — o HiView não escuta nada em nenhum outro momento. Com a opção desligada (padrão), o vídeo sai exatamente como antes, sem trilha de som.

**Se o microfone faltar:** a gravação nunca é bloqueada por causa do som. Se o microfone não puder ser aberto, aparece o aviso "Microfone indisponível: gravando sem áudio" e o vídeo é gravado normalmente, mudo. Se o microfone for desconectado **no meio** da gravação, o vídeo é finalizado na hora com um aviso — o arquivo fica válido, com som até o ponto em que o microfone parou.

---

## 9. Galeria

Abra com o botão **Galeria** no topo da janela ou `Ctrl+G`. Volte ao
vídeo ao vivo pela seta **←** do cabeçalho — o microscópio continua
capturando enquanto a galeria está aberta, então a volta é imediata.

A galeria mostra as fotos da pasta configurada em Preferências (padrão
`Imagens/HiView`) e os vídeos de `Vídeos/HiView`, dos mais novos aos
mais antigos. Cada item mostra miniatura e data/hora; vídeos mostram
também um selo de ▶ e a duração. Fotos e vídeos feitos com o app
aparecem na galeria automaticamente, sem precisar reabri-la.

- **Clicar** em um item abre no aplicativo do sistema.
- **Setas do teclado** navegam entre os itens e **Enter** abre o item
  selecionado.
- **Botão direito** em um item abre o menu: **Abrir**,
  **Mostrar na pasta** e **Mover para a lixeira** (pede confirmação;
  o arquivo pode ser recuperado da lixeira do sistema).

Enquanto uma gravação está em andamento, o indicador vermelho aparece
também no cabeçalho da galeria e o vídeo em gravação aparece como um
item "Gravando…" no topo — ele não pode ser aberto nem excluído até a
gravação terminar.

Um arquivo que não puder ser lido aparece com um ícone de alerta, sem
atrapalhar os demais itens.

---

## 10. Botão físico Snap

Com o componente opcional instalado (seção 3.2):

| Ação no botão | Resultado |
|---|---|
| Clique curto | Tira foto |
| Segurar ~1 segundo | Inicia/para a gravação |

Você pode mudar o tempo do clique longo em Preferências → **Botão físico** (padrão: 700 ms). A mesma página mostra o status do botão: **"Conectado via driver"** (leitura direta do kernel), **"Conectado via serviço"** (daemon `hiview-buttond`) ou **"Não disponível"**. Um aviso (toast) também aparece na janela quando o botão conecta ou desconecta.

> O botão só funciona com o HiView aberto e com o microscópio transmitindo imagem.

---

## 11. Modo Bancada (eletrônica)

Ative pelo seletor do topo ou pela tecla `M`. O painel lateral mostra
as ferramentas abaixo; todas as escolhas ficam salvas para a próxima
vez que você abrir o programa.

### 11.1 Grade e mira

- **Grade** (`G`): linhas de referência para alinhar componentes.
  No painel você ajusta o **espaçamento**, a **cor**
  e a **opacidade**. As linhas acompanham zoom, arrasto, espelhamento e
  rotação, e ficam sempre finas e nítidas, mesmo em 800%.
- **Mira** (`C`): marca o centro da imagem. Estilos: **cruz**,
  **cruz com círculo** e **cruz com escala** (traços de espaçamento
  regular). A cor também é configurável no painel.

> O espaçamento da grade e da escala da mira segue a calibração: com um
> perfil ativo (§11.3), o painel edita em **milímetros**; sem
> calibração, em pixels da imagem.

Grade e mira aparecem na tela e, se **Gravar sobreposições** estiver
ligada em Preferências → Fotos, também saem nas fotos. Os vídeos nunca
gravam sobreposições.

### 11.2 Ajustes de imagem

Brilho, contraste, saturação, gama e nitidez, em cinco deslizadores no
painel lateral. O efeito muda na hora, sem interromper o vídeo, e os
ajustes entram também nas **fotos** e nos **vídeos** — tudo sai igual à
tela.

- **Restaurar padrões** desfaz tudo e volta a imagem ao normal.
- Os ajustes ficam salvos **por aparelho** e voltam ao reconectar ou
  reabrir o app.
- Os ajustes são feitos **por software**, dentro do próprio app, e
  não dependem de nenhum componente extra do sistema.

> O brilho dos **LEDs** do microscópio só muda pela roda do cabo. O
> ajuste de brilho do app clareia ou escurece a imagem, mas não muda a
> luz. Para objetos que refletem muito, baixe primeiro os LEDs.

### 11.3 Medição

> ⚠️ **Antes de medir, calibre.** Sem calibração, as medidas saem em pixels.

**Como calibrar:**

1. Coloque sob o microscópio uma **lâmina de calibração** e foque. A melhor referência é a **cruz com divisões de 0,1 mm**; nas ampliações menores, use a régua de 40 mm da lâmina. Sem lâmina, uma régua milimetrada comum também serve, com menos precisão.
2. Painel Bancada → Medição → **Nova calibração…**.
3. Desenhe uma linha sobre uma distância conhecida, de preferência longa (por exemplo, 10 divisões da cruz = 1,0 mm). Quanto maior a distância, menor o erro.
4. Digite a distância real (ex.: `1` mm) e dê um nome ao perfil (ex.: "Anel no máximo, altura baixa").
5. Pronto: as medidas passam a sair em mm/µm.

Os perfis ficam salvos no painel Bancada → **Medição**: você pode
selecionar o perfil ativo, renomeá-lo ou excluí-lo.

> ⚠️ **Muito importante:** a calibração vale **só para aquela posição do anel de ampliação e aquela altura**. Mexeu no anel ou no suporte? Calibre de novo ou escolha outro perfil salvo. O HiView mostra esse aviso enquanto houver uma calibração ativa.

**Ferramentas:**

| Ferramenta | Como usar |
|---|---|
| Régua (`L`) | Clique no ponto inicial e no final |
| Ângulo (`A`) | Clique em três pontos (o do meio é o vértice) |
| Círculo (`O`) | Clique no centro e arraste até a borda: mostra o diâmetro |

Enquanto uma ferramenta está armada, o **botão esquerdo** mede e a
**roda do meio arrastada** faz o pan (a roda continua dando zoom).
`Esc` cancela a medição em andamento. As medidas concluídas ficam na
lista do painel — cada uma tem um botão de excluir e **Limpar
medições** remove todas.

As medidas aparecem com a **margem de erro** (ex.: `0,52 mm ± 0,01`).
Com 640×480 pixels, cada pixel tem um tamanho real, e é isso que a
margem indica.

Dica: congele a imagem (`F`) antes de medir para ter mais precisão.

---

## 12. Solução de problemas

| Problema | O que fazer |
|---|---|
| O app não encontra nenhuma câmera | Confira se o cabo USB está bem encaixado (troque de porta). Seu usuário precisa estar no grupo `video`: rode `groups` no terminal e, se `video` não aparecer, rode `sudo usermod -aG video <seu-usuário>` e faça logout/login. Verifique também se outro app está usando a câmera (Cheese, navegador, chamada de vídeo) — feche-o e reabra o HiView |
| "Procurando microscópio…" não sai | Troque de porta USB. Teste `v4l2-ctl --list-devices` no terminal: o microscópio aparece como "HD camera" |
| "Não foi possível listar as câmeras" | Falta o plugin de vídeo do GStreamer: `sudo apt install gstreamer1.0-plugins-good` e reabra o app |
| Aparece a webcam em vez do microscópio | Menu ☰ → Câmera → escolha "Microscópio (05e3:f12a)" |
| Imagem escura | Gire a roda de brilho no cabo e depois ajuste o brilho no modo Bancada |
| Imagem borrada | Gire o anel de ampliação devagar. Objetos muito altos podem precisar de mais distância |
| Vídeo sai travado ou a câmera dá menos quadros | Em cena escura a exposição automática da câmera cai para ~20 fps — é limitação do aparelho, não do app. Ilumine a cena (roda do cabo) |
| Botão Snap não faz nada | Verifique se o pacote `hiview-buttond` está instalado e se você fez **logout/login** depois da instalação. Status em Preferências → Botão físico |
| Botão funcionava e parou | O microscópio trocou de endereço USB ao ser reconectado — o serviço acompanha sozinho; se não voltar, reinicie o app |
| "Gravação indisponível" | Confira os plugins: `sudo apt install gstreamer1.0-plugins-good gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly` |
| Vídeo sai sem som | O áudio fica **desligado por padrão**: ligue em Preferências → Vídeo → Gravar áudio do microfone, no botão de microfone da barra, ou `Shift+R` |
| Medidor de som não se mexe | Confira o microfone escolhido em Preferências → Vídeo e o volume de entrada do sistema |
| Medidas parecem erradas | Você mexeu no anel ou na altura depois de calibrar? Calibre de novo |
| Ícone não aparece no menu | Rode `update-desktop-database` ou reinicie a sessão gráfica — alguns ambientes demoram para indexar o novo `.desktop` |
| App não abre em PC muito antigo | O HiView exige processador x86-64-v2 (SSE4.2). Veja §3 |

---

## 13. Atalhos de teclado

| Tecla | Ação |
|---|---|
| `Espaço` | Tirar foto |
| `R` | Iniciar/parar gravação |
| `Shift+R` | Ligar/desligar o microfone nas gravações |
| `F` | Congelar/descongelar |
| `G` | Grade (modo Bancada) |
| `C` | Mira (modo Bancada) |
| `L` / `A` / `O` | Régua / ângulo / círculo (modo Bancada) |
| `H` / `V` | Espelhar horizontal / vertical |
| `[` / `]` | Girar −90° / +90° |
| `+` / `-` | Zoom |
| `0` / `1` | Ajustar à janela / 100% |
| `M` | Alternar Explorar/Bancada |
| `Ctrl+G` | Galeria |
| `Ctrl+,` | Preferências |
| `F11` | Tela cheia |
| `Esc` | Cancelar medição em andamento / sair da tela cheia |
| `Ctrl+?` | Lista de atalhos |
| `Ctrl+Q` | Sair |

---

## 14. Especificações do microscópio (medidas)

| Item | Valor |
|---|---|
| Identificação USB | `05e3:f12a` (Genesys Logic) |
| Resolução real do sensor | 640×480 |
| Taxa de quadros | 30 fps |
| Formato | YUYV (sem compressão) |
| Ampliação | Ajuste mecânico pelo anel |
