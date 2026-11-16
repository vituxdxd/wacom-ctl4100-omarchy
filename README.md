# wacom-ctl4100-omarchy

Driver de espaço de usuário e software companheiro, em Rust, para a mesa
digitalizadora **Wacom Intuos S (CTL-4100)** em Arch Linux / Omarchy
(Wayland / Hyprland).

O protocolo foi reconstruído a partir do instalador oficial macOS
`WacomTablet_6.4.15-2.dmg` (engenharia reversa documentada em
[`docs/RE_REPORT_CTL4100.md`](docs/RE_REPORT_CTL4100.md)) e conferido contra o
descritor HID de hardware real.

## Estado

- `wacomd` — daemon: hidraw → `/dev/uinput`, perfis por app (Hyprland), IPC.
- `wacomctl` — CLI: status, configuração, perfis, telemetria.
- `wacom-gui` — app egui: caneta, ExpressKeys, mapeamento e diagnóstico.
- `wacom-proto` — parser HID, curvas de pressão, transformação afim, config.

**Não testado em hardware nesta máquina** (a CTL-4100 não estava conectada).
O relatório separa explicitamente o que foi observado no binário/descritor do
que é implementação nossa (curvas preset, matemática de mapeamento).

## Hardware suportado

| Modelo | USB | Bluetooth |
| --- | --- | --- |
| CTL-4100 | `056a:0374` | — |
| CTL-4100WL | `056a:0376`, `056a:03c5` | `056a:0377`, `056a:03c6` |

Área ativa: 152 × 95 mm, 2540 lpi (100 unidades/mm), 4096 níveis de pressão,
4 ExpressKeys (A–D), caneta LP-1100K com 2 botões laterais.

## Instalação (Arch / Omarchy)

```bash
# 1. instalar (a regra udev usa uaccess; não é preciso entrar no grupo input)
cd packaging
makepkg -si

# 2. driver uinput + daemon na sessão
sudo modprobe uinput
systemctl --user enable --now wacomd
```

Se o `wacomd` reclamar de permissão em `/dev/uinput` ou no hidraw, aí sim:
`sudo usermod -aG input "$USER"` e saia/entre na sessão.

O pacote instala a regra udev que dá acesso ao hidraw e **desvincula** o driver
`wacom` do kernel apenas para esta mesa (evita eventos duplicados). O driver do
kernel continua disponível para outras mesas Wacom.

> Se você tiver `input:tablet:output` configurado no Hyprland, deixe-o vazio
> (`input:tablet:output =`). O `wacomd` faz o mapeamento por conta própria e
> espera que o Hyprland distribua o dispositivo pelo layout inteiro.

## Uso

```bash
wacomctl status
wacomctl listen                 # telemetria ao vivo
wacomctl set pressure 30        # 0 = soft, 100 = firm
wacomctl set orientation 90
wacomctl set area 0 0 1 1       # x y w h normalizados
wacomctl set proportions on
wacomctl set monitor DP-1       # ou "all"
wacomctl set action key1 '{"type":"key","value":{"mods":["ctrl"],"key":"z"}}'
wacomctl use krita              # fixa o perfil; "wacomctl use --follow" volta
wacomctl profile list
wacomctl radial
```

A GUI abre pelo menu do desktop (`Wacom Tablet`) ou por `wacom-gui`. Ela edita
a mesma configuração do daemon em tempo real.

Configuração: `$XDG_CONFIG_HOME/wacom-ctl4100/config.json`
(`~/.config/wacom-ctl4100/config.json`).

## Arquitetura

```
crates/
  wacom-proto   parser HID + feature reports + curva + mapeamento + config + IPC
  wacomd        hidapi -> uinput, Hyprland IPC, servidor Unix socket
  wacomctl      clap + cliente IPC
  wacom-gui     eframe/egui (Wayland, GPU)
packaging/      udev, systemd --user, .desktop, PKGBUILD
docs/           relatório de engenharia reversa
```

Fluxo: relatório HID `0x10` (caneta) / `0x11` (ExpressKeys) → `wacom-proto`
decodifica → curva de pressão → mapeamento para pixels do layout → eixo
absoluto normalizado 0..32767 no dispositivo virtual → libinput/Hyprland.
As ExpressKeys executam `Action` (tecla, clique, scroll, modo precisão, troca de
monitor, menu radial ou comando shell) em um teclado/mouse virtual separado.

## Verificação

```bash
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Limitações conhecidas

- Sem teste em hardware nesta sessão; ver §7 do relatório.
- A curva de pressão usa Bézier + LUT de 32 amostras, como o driver oficial,
  mas os pontos exatos dos presets não foram recuperados.
- Tilt é decodificado e exposto na telemetria, mas não emitido: a CTL-4100
  (OAK) não reporta tilt no caminho interno do driver oficial.
- O eraser é inferido do bit vendor `0x36`; a LP-1100K não tem borracha.
- Bluetooth (pacotes de 361 bytes) não é implementado; use o cabo USB.
- O modo Pan/Scroll usa o movimento da caneta como roda e o toque como clique
  esquerdo.
