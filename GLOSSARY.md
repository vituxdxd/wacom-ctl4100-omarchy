# Glossário

**Mesa (tablet)** — a superfície física da CTL-4100; 152 × 95 mm ativos.

**Espaço da mesa (tablet space)** — coordenadas nativas do firmware, X em
0…15200 e Y em 0…9500, com 100 unidades por milímetro.

**Espaço da tela (layout space)** — pixels lógicos do arranjo de monitores do
Hyprland; a origem pode ser negativa.

**Área ativa (active area)** — sub-retângulo da mesa que gera coordenadas;
expressa em fração 0…1 da mesa.

**Destino (screen target)** — retângulo do layout para onde a mesa é mapeada:
o layout inteiro ou um monitor.

**Forçar proporções (force proportions)** — manter a razão de aspecto física
da área ativa dentro do destino, centralizando o resultado.

**Orientação** — rotação aplicada à mesa antes da projeção: 0°, 90°, 180° ou
270° no sentido horário.

**Tip feel** — posição da curva de pressão entre "soft" (pouca força chega ao
máximo) e "firm" (é preciso mais força).

**Curva de pressão** — função que converte pressão crua (0…4095) em pressão
reportada, implementada como Bézier cúbica amostrada em uma tabela.

**Relatório de caneta** — input report HID `0x10`, com posição, pressão, botões
e estado de proximidade.

**Relatório de pad** — input report HID `0x11`, com o bitmask das quatro
ExpressKeys.

**Wacom mode** — estado do firmware em que ele envia os relatórios proprietários
`0x10`/`0x11`; oposto de **HID mode** (modo mouse genérico).

**ExpressKey** — uma das quatro teclas físicas no topo da mesa, rotuladas
A–D no sentido esquerda→direita.

**Ação** — comportamento atribuído a uma ExpressKey ou botão da caneta:
tecla, clique, scroll, modo precisão, troca de monitor, menu radial ou shell.

**Perfil** — conjunto de overrides de caneta, ExpressKeys e mapeamento
associado a uma classe de aplicativo do Hyprland; o perfil global é a base.

**Modo precisão** — reduz o deslocamento do cursor a uma fração do movimento
físico, relativo à última posição emitida.

**Pan/Scroll** — modo em que o movimento da caneta gera roda vertical/horizontal
em vez de mover o cursor.

**Menu radial** — sobreposição com ações circulares, invocada por uma ação
mapeada.

**Telemetria** — amostra ao vivo do estado decodificado (X, Y, pressão, botões,
taxa de pacotes) publicada pelo daemon via IPC.
