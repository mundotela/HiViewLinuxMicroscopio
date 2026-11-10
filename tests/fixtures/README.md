# Fixtures de teste

Gravações reais do microscópio USB (`05e3:f12a`), capturadas em 2026-11-10 na
máquina do Henrique. Formato nativo do sensor: YUY2 640×480 @ 30 fps.

## Calibração (referência para os testes do M8)

`calibration_cross_0p1mm.mp4` e `calibration_cross_0p1mm.png`: lâmina de
calibração com cruz de divisões de **0,1 mm**, cruz centrada, braço horizontal
nivelado e braço vertical dentro de ~1°; foco nítido.

Metadados de calibração nesta posição do anel de zoom (anotados pelo Henrique):

- **Campo de visão:** ~7,2 × 5,4 mm
- **Escala:** ~89 px/mm (**1 px ≈ 11 µm**)

Para reproduzir a mesma escala, ajuste o anel até o campo de visão mostrar
~7,2 mm na horizontal.

## Inventário

| Arquivo | Conteúdo | Formato |
|---|---|---|
| `calibration_cross_0p1mm.mp4` | Lâmina de calibração, 5 s | H.264 constrained-baseline, 640×480 @ 30 fps |
| `calibration_cross_0p1mm.png` | Lâmina de calibração, quadro | PNG 640×480 |
| `smd_board.png` | Placa com componentes SMD | PNG 640×480 |
| `common_object.png` | Objeto comum | PNG 640×480 |

Os PNGs foram capturados descartando os primeiros quadros do sensor (o primeiro
quadro após abrir o dispositivo vem corrompido — buffer não inicializado).
