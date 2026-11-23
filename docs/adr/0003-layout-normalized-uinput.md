# 0003 — uinput coordinates normalized to the layout

## Context

Hyprland maps tablet devices to the whole layout or to a single monitor
(`input:tablet:output`). If we emitted native coordinates (0…15200), mapping to
a specific monitor would depend on that global setting.

## Decision

Emit `ABS_X`/`ABS_Y` in `0…32767`, corresponding to the whole layout, and do
monitor selection, active area, rotation and proportions in the daemon. The
daemon assumes `input:tablet:output` is empty.

## Consequences

- A single device works on any monitor arrangement.
- libinput requires non-zero resolution on the X/Y axes to accept the tablet;
  we declare 216/345 so it reports 152 × 95 mm.
- If the user sets `input:tablet:output`, the compositor rescales and the
  mapping becomes relative — that is why the README asks for it to be empty.
