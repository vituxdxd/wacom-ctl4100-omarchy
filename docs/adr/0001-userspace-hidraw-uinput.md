# 0001 — Driver de espaço de usuário em vez de driver de kernel

## Contexto

A CTL-4100 já é atendida pelo driver `wacom` do kernel Linux, mas queremos
perfis por aplicativo, mapeamento por monitor, remapeamento das ExpressKeys e
uma GUI — coisas que o driver do kernel não expõe. Escrever um módulo de kernel
torna qualquer bug um risco de tela azul e dificulta o desenvolvimento.

## Decisão

Ler o hidraw e criar dispositivos virtuais via `/dev/uinput`. O driver
`wacom` do kernel permanece ligado (neste kernel o hidraw desaparece quando o
driver HID é desvinculado). O kernel também é dono da inicialização: reenviar
`02 02`/`0D 00` pelo hidraw pode pausar o stream, então o daemon é passivo. Ele
dá `EVIOCGRAB` nos eventos evdev do kernel para não entregar a caneta duas
vezes.

## Consequências

- O daemon roda como usuário (grupo `input`), sem root.
- Precisamos reimplementar init, parsing e mapeamento — e assumir o custo de
  manter isso.
- Outras mesas Wacom continuam usando o driver do kernel.
- Sem o `EVIOCGRAB`, há eventos duplicados; o grab é liberado ao parar o
  daemon, devolvendo o controle ao driver do kernel.
- O grab exige acesso de leitura aos `/dev/input/event*`; a regra udev usa
  `uaccess` para isso.
