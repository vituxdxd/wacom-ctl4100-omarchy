# wacom-ctl4100-omarchy

Userspace driver and companion software, written in Rust, for the **Wacom
Intuos S (CTL-4100)** drawing tablet on Arch Linux / Omarchy (Wayland /
Hyprland).

The protocol was reconstructed from the official macOS installer
`WacomTablet_6.4.15-2.dmg` (reverse engineering documented in
[`docs/RE_REPORT_CTL4100.md`](docs/RE_REPORT_CTL4100.md)) and checked against
the HID descriptor of real hardware.

## Status

- `wacomd` — daemon: hidraw → `/dev/uinput`, per-app profiles (Hyprland), IPC.
- `wacomctl` — CLI: status, configuration, profiles, telemetry.
- `wacom-gui` — egui app: pen, ExpressKeys, mapping and diagnostics.
- `wacom-proto` — HID parser, pressure curves, affine transform, config.

**Tested on real hardware on 2026-11-16** (CTL-4100 USB `056a:0374`): pointer,
pressure, side buttons and all 4 ExpressKeys working. The test uncovered and
fixed three details that do not show up in the macOS binary: the in-range bit
is `0x40` (not `0x20`), libinput requires non-zero resolution on the virtual
tablet axes, and the hidraw node disappears if the kernel `wacom` driver is
unbound — so the daemon keeps the kernel driver bound and uses `EVIOCGRAB`.
The report separates what was observed from what is our own implementation
(curve presets, mapping math).

## Supported hardware

| Model | USB | Bluetooth |
| --- | --- | --- |
| CTL-4100 | `056a:0374` | — |
| CTL-4100WL | `056a:0376`, `056a:03c5` | `056a:0377`, `056a:03c6` |

Active area: 152 × 95 mm, 2540 lpi (100 units/mm), 4096 pressure levels,
4 ExpressKeys (A–D), LP-1100K pen with 2 side buttons.

## Installation (Arch / Omarchy)

```bash
# 1. install (the udev rule uses uaccess; no input group needed)
cd packaging
makepkg -si

# 2. uinput driver + daemon in the session
sudo modprobe uinput
systemctl --user enable --now wacomd
```

If `wacomd` complains about permissions on `/dev/uinput` or the hidraw node,
then: `sudo usermod -aG input "$USER"` and log out and back in.

`wacomd` keeps the kernel `wacom` driver bound (the hidraw node only exists
while a HID driver is bound). Initialization (`02 02`, Wacom mode, packet
rate) is done by the kernel driver; the daemon **sends no feature reports** so
it does not pause the stream. It reads hidraw and issues **EVIOCGRAB** on this
tablet's evdev nodes. That way the pen does not produce duplicate events.
When the daemon stops, the grabs are released and the kernel driver works on
its own again.

The virtual stylus identifies as `1209:0001` (`CTL-4100 Userspace Pen`) and
declares the axes with a resolution of 216/345 units (152 × 95 mm over
`0..32767`); without non-zero resolution libinput ignores the tablet. The
`wacom-gui` uses asynchronous IPC, so saving configuration or switching
profiles does not freeze the window.

> If you have `input:tablet:output` configured in Hyprland, leave it empty
> (`input:tablet:output =`). `wacomd` does the mapping itself and expects
> Hyprland to spread the device across the whole layout.

## Usage

```bash
wacomctl status
wacomctl listen                 # live telemetry
wacomctl set pressure 30        # 0 = soft, 100 = firm
wacomctl set orientation 90
wacomctl set area 0 0 1 1       # x y w h normalized
wacomctl set proportions on
wacomctl set monitor DP-1       # or "all"
wacomctl set action key1 '{"type":"key","value":{"mods":["ctrl"],"key":"z"}}'
wacomctl use krita              # pin the profile; "wacomctl use --follow" resumes
wacomctl profile list
wacomctl radial
```

The GUI opens from the desktop menu (`Wacom Tablet`) or via `wacom-gui`. It
edits the daemon's configuration in real time.

Configuration: `$XDG_CONFIG_HOME/wacom-ctl4100/config.json`
(`~/.config/wacom-ctl4100/config.json`).

## Architecture

```
crates/
  wacom-proto   HID parser + feature reports + curve + mapping + config + IPC
  wacomd        hidapi -> uinput, Hyprland IPC, Unix socket server
  wacomctl      clap + IPC client
  wacom-gui     eframe/egui (Wayland, GPU)
packaging/      udev, systemd --user, .desktop, PKGBUILD
docs/           reverse engineering report
```

Flow: HID report `0x10` (pen) / `0x11` (ExpressKeys) → `wacom-proto`
decodes → pressure curve → mapping to layout pixels → normalized absolute
axis 0..32767 on the virtual device → libinput/Hyprland.
ExpressKeys run an `Action` (key, click, scroll, precision mode, monitor
switch, radial menu or shell command) on a separate virtual keyboard/mouse.

## Verification

```bash
cargo check --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Known limitations

- Tested on hardware; see §7 of the report for what was observed and what
  remains inferred.
- The pressure curve uses Bézier + a 32-sample LUT, like the official driver,
  but the exact preset points were not recovered.
- Tilt is decoded and exposed in telemetry but not emitted: the CTL-4100
  (OAK) does not report tilt on the official driver's internal path.
- The LP-1100K has no eraser; bit `0x36` is in-range and there is no eraser
  bit in the captured reports.
- Bluetooth (361-byte packets) is not implemented; use the USB cable.
- Pan/Scroll mode uses pen movement as a wheel and the tip as a left click.

## Legal

This is an independent interoperability project. It is not affiliated with,
endorsed by or supported by Wacom Co., Ltd. "Wacom" and "Intuos" are trademarks
of Wacom Co., Ltd.

No proprietary Wacom code, binaries or assets are included in this repository.
The reverse engineering report contains only observed facts, symbol names and
addresses needed for interoperability. The extracted vendor installer used
during the research stays untracked (see `.gitignore`).

## License

MIT — see [LICENSE](LICENSE).
