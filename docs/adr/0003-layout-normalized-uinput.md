# 0003 — Coordenadas do uinput normalizadas no layout

## Contexto

O Hyprland mapeia dispositivos de mesa para o layout inteiro ou para um monitor
(`input:tablet:output`). Se emitíssemos coordenadas nativas (0…15200), o
mapeamento para um monitor específico dependeria dessa configuração global.

## Decisão

Emitir `ABS_X`/`ABS_Y` em `0…32767`, correspondendo ao layout inteiro, e fazer
no daemon a seleção de monitor, área ativa, rotação e proporções. O daemon
assume que `input:tablet:output` está vazio.

## Consequências

- Um único dispositivo funciona em qualquer arranjo de monitores.
- O libinput exige resolução não-zero nos eixos X/Y para aceitar o tablet;
  declaramos 216/345 para que ele reporte 152 × 95 mm.
- Se o usuário configurar `input:tablet:output`, o compositor reescala e o
  mapeamento fica relativo — por isso o README pede para deixar vazio.
