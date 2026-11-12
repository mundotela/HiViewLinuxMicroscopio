# HiView — Manual do Usuário

> **Rascunho 0.1** — descreve o comportamento planejado da versão 1.0. Atualizado a cada etapa do desenvolvimento.

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

### 3.1 Programa principal

```bash
sudo apt install ./hiview_<versão>_amd64.deb
```

Depois, procure **HiView** no menu de aplicativos.

### 3.2 Botão físico Snap (opcional)

O sistema Linux atual não reconhece o botão Snap deste microscópio sozinho. O pacote extra `hiview-buttond` resolve isso:

```bash
sudo apt install ./hiview-buttond_<versão>_amd64.deb
```

**Depois de instalar, saia da sessão e entre novamente** (logout/login). Isso é necessário para liberar o acesso ao botão.

> **Sobre segurança:** esse componente roda com privilégios de administrador porque precisa observar a comunicação USB. Ele foi feito para enviar ao HiView **somente** "botão apertado" e "botão solto", e não tem acesso à rede. Se preferir não instalar, tudo funciona normalmente pelo teclado e pelo mouse.

---

## 4. Primeiro uso

1. Conecte o microscópio a uma porta USB.
2. Abra o HiView. Ele encontra o microscópio sozinho, mesmo que o notebook tenha webcam.
3. Coloque o objeto sob a lente e gire o **anel de ampliação** até focar.
4. Ajuste o brilho na **roda do cabo**.

Se aparecer **"Procurando microscópio…"**, veja a seção 12.

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
- **Bancada** é o modo de inspeção: abre um **painel lateral** com as
  ferramentas de grade e mira (medição e ajustes de imagem chegam em
  versões futuras). As sobreposições ligadas na Bancada voltam a
  aparecer quando você retorna ao modo — mas só aparecem na Bancada.

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
| Gravar sobreposições | Inclui grade e mira na foto, iguais à tela |

---

## 8. Vídeos

- Mude para **Vídeo** na barra flutuante e clique no botão redondo, ou aperte `R` a qualquer momento.
- Um indicador vermelho mostra o tempo de gravação.
- Para parar, clique de novo ou aperte `R`.
- Os vídeos vão para **Vídeos/HiView**, em MP4 (abrem em qualquer player e celular).
- Durante a gravação, **girar** e **congelar** ficam desativados.
- Cada minuto ocupa cerca de 15–20 MB. O HiView avisa se o disco estiver quase cheio.

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

Você pode mudar o tempo do clique longo em Preferências → Botão físico. Lá também aparece o status do botão: **"conectado"** ou **"não disponível"**.

> O botão só funciona com o HiView aberto.

---

## 11. Modo Bancada (eletrônica)

Ative pelo seletor do topo ou pela tecla `M`. O painel lateral mostra
as ferramentas abaixo; todas as escolhas ficam salvas para a próxima
vez que você abrir o programa.

### 11.1 Grade e mira

- **Grade** (`G`): linhas de referência para alinhar componentes.
  No painel você ajusta o **espaçamento em pixels da imagem**, a **cor**
  e a **opacidade**. As linhas acompanham zoom, arrasto, espelhamento e
  rotação, e ficam sempre finas e nítidas, mesmo em 800%.
- **Mira** (`C`): marca o centro da imagem. Estilos: **cruz**,
  **cruz com círculo** e **cruz com escala** (traços a cada tantos
  pixels). A cor também é configurável no painel.

> O espaçamento da grade e da escala da mira hoje é em pixels da
> imagem. Espaçamento em milímetros chega junto com a calibração.

Grade e mira aparecem na tela e, se **Gravar sobreposições** estiver
ligada em Preferências → Fotos, também saem nas fotos. Os vídeos nunca
gravam sobreposições.

### 11.2 Ajustes de imagem

> ⚠️ Planejado para uma versão futura — ainda não disponível.

Brilho, contraste, saturação, gama e nitidez. Eles são aplicados pelo HiView na imagem, então aparecem também nas fotos e nos vídeos. **Restaurar padrões** desfaz tudo. Os ajustes ficam salvos para a próxima vez.

> O brilho dos **LEDs** do microscópio só muda pela roda do cabo. O ajuste de brilho do app clareia ou escurece a imagem, mas não muda a luz. Para objetos que refletem muito, baixe primeiro os LEDs.

### 11.3 Medição

> ⚠️ Planejado para uma versão futura — ainda não disponível.

> ⚠️ **Antes de medir, calibre.** Sem calibração, as medidas saem em pixels.

**Como calibrar:**

1. Coloque sob o microscópio uma **lâmina de calibração** e foque. A melhor referência é a **cruz com divisões de 0,1 mm**; nas ampliações menores, use a régua de 40 mm da lâmina. Sem lâmina, uma régua milimetrada comum também serve, com menos precisão.
2. Painel Bancada → **Calibrar**.
3. Desenhe uma linha sobre uma distância conhecida, de preferência longa (por exemplo, 10 divisões da cruz = 1,0 mm). Quanto maior a distância, menor o erro.
4. Digite a distância real (ex.: `1` mm) e dê um nome ao perfil (ex.: "Anel no máximo, altura baixa").
5. Pronto: as medidas passam a sair em mm/µm.

> ⚠️ **Muito importante:** a calibração vale **só para aquela posição do anel de ampliação e aquela altura**. Mexeu no anel ou no suporte? Calibre de novo ou escolha outro perfil salvo. O HiView mostra esse aviso enquanto houver uma calibração ativa.

**Ferramentas:**

| Ferramenta | Como usar |
|---|---|
| Régua (`L`) | Clique no ponto inicial e no final |
| Ângulo | Clique em três pontos (o do meio é o vértice) |
| Círculo | Clique no centro e arraste até a borda: mostra o diâmetro |

`Esc` cancela a medição em andamento. As medidas aparecem com a **margem de erro** (ex.: `0,52 mm ± 0,01`). Com 640×480 pixels, cada pixel tem um tamanho real, e é isso que a margem indica.

Dica: congele a imagem (`F`) antes de medir para ter mais precisão.

---

## 12. Solução de problemas

| Problema | O que fazer |
|---|---|
| "Procurando microscópio…" não sai | Troque de porta USB. Teste `v4l2-ctl --list-devices` no terminal: o microscópio aparece como "HD camera" |
| Aparece a webcam em vez do microscópio | Menu ☰ → Câmera → escolha "Microscópio (05e3:f12a)" |
| Imagem escura | Gire a roda de brilho no cabo e depois ajuste o brilho no modo Bancada |
| Imagem borrada | Gire o anel de ampliação devagar. Objetos muito altos podem precisar de mais distância |
| Botão Snap não faz nada | Verifique se o `hiview-buttond` está instalado e se você fez logout/login. Status em Preferências → Botão físico |
| "Gravação indisponível" | Instale os codecs: `sudo apt install gstreamer1.0-plugins-ugly` |
| Medidas parecem erradas | Você mexeu no anel ou na altura depois de calibrar? Calibre de novo |

---

## 13. Atalhos de teclado

| Tecla | Ação |
|---|---|
| `Espaço` | Tirar foto |
| `R` | Iniciar/parar gravação |
| `F` | Congelar/descongelar |
| `G` | Grade (modo Bancada) |
| `C` | Mira (modo Bancada) |
| `H` / `V` | Espelhar horizontal / vertical |
| `[` / `]` | Girar −90° / +90° |
| `+` / `-` | Zoom |
| `0` / `1` | Ajustar à janela / 100% |
| `M` | Alternar Explorar/Bancada |
| `Ctrl+G` | Galeria |
| `Ctrl+,` | Preferências |
| `F11` | Tela cheia |
| `Esc` | Sair da tela cheia / cancelar |
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
