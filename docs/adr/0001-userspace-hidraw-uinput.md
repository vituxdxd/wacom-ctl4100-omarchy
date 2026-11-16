# 0001 — Driver de espaço de usuário em vez de driver de kernel

## Contexto

A CTL-4100 já é atendida pelo driver `wacom` do kernel Linux, mas queremos
perfis por aplicativo, mapeamento por monitor, remapeamento das ExpressKeys e
uma GUI — coisas que o driver do kernel não expõe. Escrever um módulo de kernel
torna qualquer bug um risco de tela azul e dificulta o desenvolvimento.

## Decisão

Ler o hidraw e criar dispositivos virtuais via `/dev/uinput`. A regra udev
desvincula o `wacom` do kernel apenas para os PIDs da CTL-4100.

## Consequências

- O daemon roda como usuário (grupo `input`), sem root.
- Precisamos reimplementar init, parsing e mapeamento — e assumir o custo de
  manter isso.
- Outras mesas Wacom continuam usando o driver do kernel.
- Se o `wacom` do kernel não for desvinculado, há eventos duplicados.
