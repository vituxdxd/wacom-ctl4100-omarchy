# Glossary

**Tablet** — the physical surface of the CTL-4100; 152 × 95 mm active.

**Tablet space** — native firmware coordinates, X in 0…15200 and Y in
0…9500, with 100 units per millimeter.

**Layout space** — logical pixels of the Hyprland monitor arrangement; the
origin can be negative.

**Active area** — sub-rectangle of the tablet that produces coordinates;
expressed as a 0…1 fraction of the tablet.

**Screen target** — the layout rectangle the tablet is mapped to: the whole
layout or a single monitor.

**Force proportions** — keep the physical aspect ratio of the active area
inside the target, centering the result.

**Orientation** — rotation applied to the tablet before projection: 0°, 90°,
180° or 270° clockwise.

**Tip feel** — position of the pressure curve between "soft" (little force
reaches the maximum) and "firm" (more force is needed).

**Pressure curve** — function that converts raw pressure (0…4095) into
reported pressure, implemented as a cubic Bézier sampled into a table.

**Pen report** — HID input report `0x10`, with position, pressure, buttons
and proximity state.

**Pad report** — HID input report `0x11`, with the bitmask of the four
ExpressKeys.

**Wacom mode** — firmware state in which it sends the proprietary
`0x10`/`0x11` reports; the opposite of **HID mode** (generic mouse mode).

**ExpressKey** — one of the four physical keys on top of the tablet, labeled
A–D from left to right.

**Action** — behavior assigned to an ExpressKey or pen button: key, click,
scroll, precision mode, monitor switch, radial menu or shell.

**Profile** — set of pen, ExpressKeys and mapping overrides associated with a
Hyprland application class; the global profile is the base.

**Precision mode** — reduces cursor displacement to a fraction of physical
movement, relative to the last emitted position.

**Pan/Scroll** — mode in which pen movement generates a vertical/horizontal
wheel instead of moving the cursor.

**Radial menu** — overlay with circular actions, invoked by a mapped action.

**Telemetry** — live sample of the decoded state (X, Y, pressure, buttons,
packet rate) published by the daemon over IPC.
