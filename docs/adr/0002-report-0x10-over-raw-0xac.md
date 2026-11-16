# 0002 — Usar o Report `0x10` estruturado, não o stream `0xAC`

## Contexto

O driver macOS lê o relatório USB de 27 bytes como sub-relatórios de 8 bytes
(`TranslateShortEMR`) e também tem um caminho de stream vendor `0xAC` de 191
bytes. O descritor HID, por outro lado, define campos claros para o Report
`0x10`: X/Y de 24 bits, pressão de 16 bits, flags de ponta/botões.

## Decisão

Parsear o Report `0x10` conforme o descritor. O `0xAC` e o caminho de 8 bytes
ficam documentados, mas não implementados.

## Consequências

- O parser é simples, testável e igual ao que o driver `wacom` do Linux usa.
- Perdemos qualquer informação que só exista no stream raw (se houver).
- Se um firmware futuro só enviar `0xAC`, será preciso um segundo parser.
