# Reverse engineering report — Wacom Intuos S (CTL-4100)

Analyzed version: **Wacom Tablet 6.4.15-2** (macOS), installer
`WacomTablet_6.4.15-2.dmg`.

This document separates three kinds of claim:

- **[OBS]** observed directly in the installer, in the HID descriptor of real
  hardware or in the Linux kernel source;
- **[INF]** inference from symbols/decompilation, without execution on
  hardware on this machine;
- **[IMPL]** decision of our Rust reimplementation, when the official binary
  does not expose the exact value.

The tablet was not connected during this work. No claim below depends on a
live capture made here.

---

## 1. Extraction chain

### 1.1 Hashes and sizes

| Artifact | Size | SHA-256 |
| --- | ---: | --- |
| `WacomTablet_6.4.15-2.dmg` | 197,196,999 B | `84af01a189e7416b01c2f37c51e3f47ae16de60b6211bfc6cdd190d2567c90c0` |
| `Install Wacom Tablet.pkg` | 194,261,184 B | `d507ec51699e01641800cd91dccbc0d0bb1157a4cc45b7d20f487f3f4000e95c` |
| `WacomTabletDriver` (x86_64 slice) | 24,909,936 B | `49eae096793585f7b4dafbab2976e40075b4b2bf2bfcaafc490944c21c04e3f3` |
| HID descriptor `0003:056A:0374.0005.hid.bin` | 759 B | `a97d2c3ef0d3e50f9ad84e0cae3e2a0fe51db5d334e8be0c815735c15008ad55` |

### 1.2 Layers

```
.dmg (UDIF, Apple DMG v4)
  └─ 7z x                → "Install Wacom Tablet.pkg"  (xar)
       └─ xar -xf        → content.pkg/{Bom,PackageInfo,Payload,Scripts}
            └─ Payload   → gzip (1f 8b 08 00)
                 └─ gunzip | cpio -idm
                      → 6,479 regular files, 391 MB
```

- **[OBS]** `7z l` lists the DMG with a single `.pkg` and 5 files in total.
- **[OBS]** `xar -tf` lists 61 entries; `PackageInfo` declares
  `payload numberOfFiles="6790" installKBytes="388471"`.
- **[OBS]** the `Payload` starts with `1f 8b 08 00` (gzip), not `pbzx`; the
  `pbzx` tool was not needed.
- **[OBS]** two entries under `/Library/LaunchDaemons` could not be written
  without root; the rest was extracted.
- **[OBS]** `Library/Extensions/` is **empty**: version 6.4.15 installs no
  `.kext`. The driver is a userspace process (`WacomTabletDriver.app`) talking
  IOHID, plus privileged helpers (`Wacom_IOManager`,
  `com.wacom.UpdateHelper`, `com.wacom.DataStoreMgr`).

### 1.3 Relevant bundles

| Path | ID | Role |
| --- | --- | --- |
| `.Tablet/WacomTabletDriver.app` | `com.wacom.wacomtablet` | HID driver + logic for all tablets (49 MB) |
| `.Tablet/TabletDriver.app` | `com.wacom.TabletDriver` | launcher/agent |
| `Wacom Center.app` + `WacomCenter.prefpane` | `com.wacom.WacomCenter` | new configuration center |
| `WacomTablet.prefpane` | `com.wacom.ProfessionalControlPanel` | legacy "Wacom Tablet Properties" |
| `.Tablet/FirmwareUpdater.app` | `com.wacom.FirmwareUpdater` | firmware update |
| `.Tablet/WacomADCV.app` | `com.wacom.WacomADCV` | diagnostics/ADC |
| `.Tablet/WacomTouchDriver.app` | `com.wacom.WacomTouchDriver` | touch (not applicable to the CTL-4100) |
| `Wacom Display Settings.app` | `com.wacom.Wacom-Display-Settings` | screen mapping |

### 1.4 Analyzed binary

`WacomTabletDriver` is an `x86_64` + `arm64` fat binary. The x86_64 slice has
**122,654 defined symbols** (it was not stripped), including full class names
and source paths:

```
/Users/wacom/Development/R_6.4.15/Wacom/Mac/Tools/buildbot/R_6.4.15/
  Wacom/Common/TabletDriver/CCTL4100GraphicsTablet.cpp
  Wacom/Common/TabletDriver/CCTL4100Pen.cpp
  Wacom/Common/TabletDriver/CCTL4100TabletControls.cpp
```

Tools: `llvm-lipo -thin x86_64`, `llvm-nm -C`, `llvm-objdump`, `strings`,
Ghidra 12.2 headless (`DumpDecomp.java`, decompilation by symbol name). The
Ghidra project reused is at `/tmp/opencode/ghidra_wacom`.

---

## 2. HID / USB

### 2.1 Identification

| Item | Value | Evidence |
| --- | --- | --- |
| VID | `0x056A` (Wacom) | `lsusb`/descriptor |
| PID CTL-4100 | `0x0374` | descriptor, libwacom, `dmesg` |
| PID CTL-4100WL USB | `0x0376`, `0x03C5` | libwacom |
| PID CTL-4100WL BT | `0x0377`, `0x03C6` | libwacom |
| Product | `Intuos S` | USB string |
| Class | HID v1.10, full-speed | `dmesg` from real hardware |
| Interface | 1 HID interface, 1 IN endpoint | descriptor |

**[OBS]** The PID `0x0375` mentioned in the request does not appear in the
libwacom database nor in the descriptor. The real IDs in the CTL-4100WL line
are `0x0376/0x0377` (first generation) and `0x03C5/0x03C6` (refresh). The
daemon opens all three USB PIDs (`0374/0376/03C5`); the Bluetooth PIDs are
recognized in the report but not implemented.

### 2.2 Report descriptor

Real CTL-4100 descriptor (`Wacom Intuos S (3rd-gen)`, dump
`0003:056A:0374.0005.hid.txt`, 759 bytes), vendor page collection `0xFF0D`,
`Usage (0x01)`, `Report ID 16`:

| Report ID | Type | Payload | Purpose |
| ---: | --- | ---: | --- |
| `0x10` (16) | Input | 26 B | pen (position, pressure, buttons, tilt) |
| `0x11` (17) | Input | 8 B | 4 ExpressKeys + padding |
| `0x13` (19) | Input | 8 B | battery (wireless models) |
| `0xAC` (172) | Input | 191 B | vendor "raw" stream used by the macOS driver |
| `0x02` (2) | Feature | 1 B | device mode (HID ↔ Wacom) |
| `0x0C` (12) | Feature | 8 B | offsets/area (4 × u16) |
| `0x0D` (13) | Feature | 1 B | pen scan |
| `0x14` (20) | Feature | 13 B | serial number |
| `0x31` (49) | Feature | 3 B | LED brightness (the driver sends 5 arguments) |
| `0xCD` (205) | Feature | 2 B | hardware reset (the driver sends 3 bytes) |

The descriptor defines Feature Reports 2, 3, 4, 7, 12, 13, 20, 49, 204, 205
and several vendor blobs (21, 51, 68, 69, 96–111, 172, 208–228). We only use
the ones with confirmed semantics.

**[OBS]** The binary also sends report IDs `0x0E` (touch scan), `0x32` (ring
LED), `0x34` (mass storage) and `0x82` (Bluetooth status). These IDs do not
appear in this revision's USB descriptor: they belong to models with
touch/BT. The wired CTL-4100 does not use them, and our driver does not send
them.

### 2.3 Input Report `0x10` — pen

26-byte payload (the hidraw buffer is 27, including the Report ID):

| Offset | Bits | Field | Range / semantics |
| ---: | ---: | --- | --- |
| 0 | 1 | `TipSwitch` (usage `0x42`) | 1 = tip touching |
| 0 | 1 | `BarrelSwitch` (`0x44`) | 1 = lower side button |
| 0 | 1 | `BarrelSwitch2` (`0x5A`) | 1 = upper side button |
| 0 | 2 | constant | — |
| 0 | 1 | vendor (`0x32`) | set on hover and with the tip touching; unused |
| 0 | 1 | `InRange` (`0x36`) | 1 = pen in range (verified in capture) |
| 0 | 1 | constant | — |
| 1–3 | 24 | `X` LE | 0…15200 (100 units/mm) |
| 4–6 | 24 | `Y` LE | 0…9500 |
| 7–8 | 16 | `TipPressure` LE | 0…4095 (256 levels per useful bit) |
| 9–14 | 48 | constant | 6 bytes |
| 15 | 8 | `Z` / distance | 0…63 |
| 16–19 | 32 | `TiltX` (i32) | unused by the CTL-4100 (OAK) |
| 20–23 | 32 | `TiltY` (i32) | unused by the CTL-4100 (OAK) |
| 24–25 | 16 | vendor `0x77` | unused |

**[OBS]** The descriptor fixes `Logical Maximum 15200` for X, `9500` for Y and
`4095` for pressure.

**[OBS]** Real CTL-4100 capture (`hidraw`, 1,496 27-byte reports):

```text
hover (0x40): 10 40 93 1b 00 5b 0d 00 00 00 00 00 00 00 00 00 3f ...
tip   (0x61): 10 61 e9 23 00 f9 18 00 ab 00 00 00 00 00 00 00 12 ...
```

Bit `0x40` (bit 6) is the proximity indicator; `0x20` (bit 5) appears during
hover/tip but is not eraser. Bit `0x01` correlates exactly with pressure > 0.
The LP-1100K has no eraser: there is no inversion bit in that byte. This
corrects the initial reading of the descriptor, which attributed `InRange` to
bit 5.
**[OBS]** `CGD16StylusTransducer::HandleTransducerData` (0x10023C642) reads
pressure at `packet[8] | packet[9] << 8`, exactly offset 7–8 of the payload,
and discards tilt when `IS_OAK(sensorID)` is true.
**[OBS]** `CCTL4100GraphicsTablet::DeviceStart` (0x1000DFEBE) calls
`SetMaxPressureLevels(0x1000)`.
**[OBS]** `CCTL4100GraphicsTablet::Init` (0x1000DFC62) sets
`SetPacketSize(0x1B)` = 27 bytes in USB mode and `0x169` = 361 bytes in
Bluetooth mode.

### 2.4 Input Report `0x11` — ExpressKeys

| Offset | Bits | Field |
| ---: | ---: | --- |
| 0 | 0–3 | ExpressKeys A, B, C, D (bit 0 = A) |
| 0 | 4–7 | constant |
| 1–7 | 56 | constant (7 bytes) |

**[OBS]** `CCTL4100TabletControls::ProcessCurrentTagList` (0x1003E80DE) does
`packet[1] & 0xF`, publishes it in `ERawTabletButtons` and compares it with
the previous state to generate the HUD.

### 2.5 Input Report `0x13` — battery (WL)

| Offset | Bits | Field |
| ---: | ---: | --- |
| 0 | 0–6 | level 0…100 |
| 0 | 7 | charging |
| 1 | 8 | unknown |
| 2–7 | 48 | constant |

### 2.6 Feature reports and initialization sequence

The macOS driver sends feature reports with
`IOHIDDeviceSetReport(kIOHIDReportTypeFeature)`; the first byte of the vector
is the Report ID. Evidence:

- **[OBS]** strings `_IOHIDDeviceSetReport` and
  `CMacHIDCommInterface::SendUSBCommand:EFeatureSetReport` in the binary.
- **[OBS]** `CMacHIDCommInterface::SendUSBCommand` (0x1000A3000) passes
  `vector.data()` straight to `MacHidDevice::SendFeature`, with a retry loop.
- **[OBS]** `MacHidDevice::SetToWacomMode` (0x1002E0416) builds
  `{0x02, 0x02}`; `MacHidDevice::SetToHIDMode` (0x1002E0364) builds
  `{0x02, 0x01}`.
- **[OBS]** `CUSBCommInterface::SetToHIDMode` (0x100397C9E) also sends
  `{0x02, 0x01}`.

Sequence adopted (and implemented in `wacomd`):

```text
1. open hidraw (056a:0374)
2. SET_FEATURE 02 02        # leave mouse/HID mode, enter Wacom mode
3. wait ~40 ms
4. SET_FEATURE 0d 00        # enable pen scan (0 = on, 1 = off)
5. read input reports 0x10 / 0x11
```

Feature commands confirmed in the binary:

| Bytes | Function | Symbol / address |
| --- | --- | --- |
| `02 02` | Wacom mode | `MacHidDevice::SetToWacomMode` 0x1002E0416 |
| `02 01` | HID/mouse mode | `MacHidDevice::SetToHIDMode` 0x1002E0364 |
| `0D 00/01` | pen scan on/off | `GD16SetScanModePen` 0x100399302 |
| `0E 00/01` | touch scan on/off | `GD16SetScanModeTouch` 0x10039939C |
| `CD A6 50` | hardware reset | `GD16SendHardwareReset` 0x100399DDE |
| `31 ...` | LED brightness (5 bytes) | `GD16SetLEDBrightness` 0x100399214 |
| `32 ...` | ring mode LED (2 bytes) | `GD16SetRingModeLED` 0x10039916A |
| `34 ...` | mass storage (2 bytes) | `GD16SetMassStorage` 0x100399636 |
| `14 00…` | serial query (14 bytes) | `GD16GetSerialNumber` 0x100399436 |
| `03 00` | OEM report (2 bytes) | `GD16GetOEMReport` 0x1003994B6 |
| `0C 00…` | offsets (9 bytes) | `GD16GetOffsetReport` 0x100399536 |
| `16 00…` | Bluetooth info (15 bytes) | `GD16GetBluetoothInformation` 0x1003995B6 |
| `82 …` | Bluetooth status (49/50 bytes) | `GD16BTGetStatus` / `GD16BTSetStatus` |

**[OBS]** The Linux kernel confirms the same mode command:
`wacom_hid_set_device_mode()` (drivers/hid/wacom_sys.c) looks for the feature
field with usage `HID_DG_INPUTMODE` (Report ID 2 of the descriptor), writes the
value **2** and issues `HID_REQ_SET_REPORT`. Then `wacom_set_device_mode()`
sends `rep_data[0] = mode_report (2)`, `rep_data[1] = mode_value (2)` and
reads the report back. The CTL-4100 descriptor declares `Logical Minimum (2)`
and `Logical Maximum (2)` for that field — the value 2 is the expected mode.

**[INF]** `SetToHIDMode` sends 1, a value outside the declared min/max; this is
accepted by the firmware but is not used by our driver.

### 2.7 Rate, resolution and area

| Quantity | Value | Evidence |
| --- | --- | --- |
| Resolution | 2540 lpi = 100 units/mm | descriptor (`Unit Exponent 13`, cm) and libwacom |
| Logical width | 15200 | descriptor |
| Logical height | 9500 | descriptor |
| Active area | 152 × 95 mm | descriptor |
| Pressure | 0…4095 (4096 levels) | descriptor + `DeviceStart` |
| Distance | 0…63 | descriptor (`0x0132`) |
| Rate | configurable via `ForcePacketTiming` | `CCTL4100GraphicsTablet::EffectivePacketTiming` |

**[INF]** The nominal rate is not a recovered constant: the official driver
reads `//ForcePacketTiming` from settings and converts pps → µs
(`PPS_TO_MICROSECONDS`). Our telemetry measures the effective rate (pps)
instead of imposing one.

### 2.8 "Raw" mode `0xAC`

**[OBS]** The macOS driver treats the 27-byte report as a sequence of 8-byte
sub-reports on the `HidReport::TranslateShortEMR` path (0x10040CFD9) and in
`CCTL4100GraphicsTablet::ProcessDataPacket` (0x1000E02C2). Each sub-report has
bit 7 of the first byte as a continuation flag and is expanded into a 16-byte
internal packet starting at `0x10`, with padding inserted.

**[IMPL]** We do **not** use that path. The HID descriptor defines
unambiguous structured fields for Report `0x10`, and that is how the Linux
`wacom` driver reads the tablet. The 8-byte path is kept in the report as an
observation, but the implemented parser follows the descriptor (24-bit X/Y,
16-bit pressure).

---

## 3. Official driver algorithms

### 3.1 Pressure curve

**[OBS]** `CPressureCurve` inherits from `CBezierCurve`:

- constructor `CPressureCurve::CPressureCurve(unsigned short)` (0x1001AABF6)
  calls `CBezierCurve::CBezierCurve(EBezierCurveType, int)` with type 2 and
  `0x20` (32 samples);
- `CPressureCurve::EvaluateAtValue(double)` (0x1001AADD2):
  - above the end point → returns `GetMaxPressure()` (0x1000);
  - below the start point → evaluates the lookup table (clamped at 0);
  - in between → `CLookupTableCurve::EvaluateAtValue` and `max(0, y)`;
- `CPressureCurve::EvaluateAll()` (0x1001AAE28) precomputes the table;
- `CPressureCurve::SetPointTable` accepts `SDoublePoint` pairs and selects the
  sub-table used by the Bézier;
- `CCTL4100Pen` uses `CGD16StylusTransducer`, which publishes raw pressure
  (`PutRawPressureInTaglist`) and then converts it.

**[IMPL]** Our implementation:

- cubic Bézier from `(0,0)` to `(1,1)` with two free control points;
- `TipFeel` 0…100 % interpolates between soft / linear / firm:
  - soft: `P1=(0.05,0.55)`, `P2=(0.35,0.98)`
  - linear: `P1=(1/3,1/3)`, `P2=(2/3,2/3)`
  - firm: `P1=(0.62,0.10)`, `P2=(0.95,0.45)`
- `x(t)` inversion by bisection (24 iterations) and a 33-point LUT;
- the exact preset points of the official driver **were not recovered** (they
  live in the GUI/config, not in binary constants). The Bézier + LUT
  architecture itself is the observed part.

### 3.2 Coordinates, smoothing and double click

**[OBS]** `CCTL4100Pen::HandleTransducerData` (0x1000DC39A) calls
`CTransducer::CheckDoubleClickAssist` (0x10003731EE) when byte `0x39` of the
object is 1. `CButton::FixUpPressure` (0x100021202) exists on the button path.

**[OBS]** Smoothing/rotation strings and symbols:
`PlatformDoubleClick`, `GetDoubleClickTime`,
`PressureFixedDepthFilter`, `PressureSafteyMargin`, `RotationHelper`,
`ProcessRotation`, `InterpolateRotation`, `PutRawRotationInTagList`.

**[IMPL]** Double click is implemented as a *snap*: if two tip touches occur
within `double_click_distance` pixels and `double_click_time_ms`, the second
position is replaced by the first before the pressure event. It is the
behavior produced by Wacom Center's "Tip Double Click Distance". Our default
is 10 px / 400 ms; the official driver's exact values were not extracted.

### 3.3 Spatial mapping

**[OBS]** Driver classes:

- `CMappingInputTablet::GetInputArea` (0x10030A394), `GetSelectedArea`,
  `SetAspectType`, `SetMappingOptions`, `SetIsInsetTabletModeOn`;
- `CMappingOutputScreen::GetMonitorArea` (0x100310E58),
  `CMappingOutputScreen::GetMonitor` (0x10022744C);
- `CMappingGroup` / `CMappingSet` (mapping groups per screen);
- `SMatrix` (48 methods) for the transformation.

**[IMPL]** Our transformation, in `wacom-proto::transform`:

1. normalize the native coordinate within the active area:
   `u = (x − x0)/w`, `v = (y − y0)/h`, clamp 0…1;
2. apply clockwise rotation:
   - 0°: `(u,v)`
   - 90°: `(1−v, u)`
   - 180°: `(1−u, 1−v)`
   - 270°: `(v, 1−u)`
3. if `Force Proportions`, fit a rectangle with the tablet's aspect ratio
   (152:95) inside the target, centered;
4. project onto the destination rectangle (whole layout or one monitor);
5. normalize the layout pixel to `0…32767` on the uinput absolute axis.

90° rotation swaps the ratio to 95:152, as expected. Precision ("FinePoint")
applies `prev + (next − prev) × 0.5`.

---

## 4. Functional mapping of the software

### 4.1 Mappable actions

**[OBS]** The binary contains the `ECommandType` enum with dozens of actions.
The ones relevant to the CTL-4100:

| Group | Observed actions |
| --- | --- |
| Keyboard | `EKeystroke`, `EKeystrokes`, `EModifiers`, `EKeyboardGroup`, `EUndo`, `ERedo` |
| Mouse | `ELeftClick`, `ERightClick`, `EMiddleClick`, `ELeftDoubleClick`, `ERightDoubleClick`, `EMiddleDoubleClick`, `ELeftClickLock`, `EMiddleClickLock`, `ERightClickLock` |
| Scroll/Zoom | `EScroll`, `EScrollUp`, `EScrollDown`, `EZoom`, `EZoomIn`, `EZoomOut`, `EAutoScrollZoom`, `EPenButtonPanScroll` |
| Modes | `EFinePoint`, `EModeToggle`, `EPressureHold`, `ETiltToggle`, `ETouchToggle`, `EDialToggle` |
| Screen/Window | `EDisplayToggle`, `EShowDesktop`, `EShowAllWindows`, `EShowLaunchpad`, `ESwitchApp`, `ESnapWindowLeft`, `ESnapWindowRight`, `ESwitchVDLeft`, `ESwitchVDRight`, `EShowNextFullScreenApp`, `EShowPreviousFullScreenApp` |
| Wacom | `ERadialMenu`, `EShowRadialMenuPanel`, `EShowOnScreenPanel`, `EShowGridPanel`, `EToggleWacomCenter`, `EWacomDisplaySettings`, `EToggleControlPanel` |
| Apps | `ERunApplication`, `ERunOneNote`, `ERunScreenSketch`, `ERunStickyNotes`, `EShowDictionary`, `EFileExplorer`, `EAppDefined` |
| System | `EChangeSettings`, `EInternalOSD`, `EDisplayToggle`, `EScreenRotationLockToggle`, `ERunActionCenter`, `EStartMenu` |
| Special | `EDisabled`, `ESeparator`, `ESkip`, `EJournal`, `EMacro_Deprecated` |

Classes that execute actions: `CWacomFunction_Keystroke`,
`CWacomFunction_SimpleEvent`, `CWacomFunction_RunApp`,
`CWacomFunction_RadialMenu`, `CWacomFunction_PenMouseMode`,
`CWacomFunction_EventPacket`, `CWacomFunction_Disabled`.

**[IMPL]** Our subset: key+modifiers, left/right/middle click, double click,
scroll, pan/scroll, precision mode, monitor switch, radial menu and shell
command. Window/OS actions do not make sense on Wayland through uinput; they
can be expressed as Hyprland shortcuts via `Action::Key`.

### 4.2 Wacom Center / Tablet Properties screens

**[OBS]** Prefpane and resources include `Devices.strings`,
`Help_<Model>.nib`, `Localizable.strings` (values in base64), the
`BezelWindowContent.nib`, calibration screens and the "Wacom Experience
Program". The tab structure is the same one documented by the product:

- **Pen**: pen buttons, Tip Feel (curve), test pressure, double click;
- **ExpressKeys**: 4 keys, actions, on-screen HUD, radial menu;
- **Mapping**: screen/monitor, tablet area, proportions, orientation;
- **Touch**: absent on the CTL-4100;
- **Devices / About**: firmware, diagnostics, profiles;
- **Per-application profiles**: the driver keeps `CMappingSet`/`CMappingGroup`
  per app and the panel offers "Application Specific Settings".

**[OBS]** `CCTL4100TabletControls::CreateControls` (0x1003E79A0) and
`CCTL4100GraphicsTablet::CreateTabletMenuArea` (0x1000E020C) create the
tablet's control area; `ProcessEventForType`/`GetNthControlForType` handle the
four buttons. The key HUD is controlled by `GetShowHUD` and by element `0x8D`
of the tag list.

### 4.3 Real-time diagnostics

**[OBS]** The binary has `CGD16GraphicsTablet::HexDump`, packet counters
(`SetPacketCount`, `SetPacketPosition`), `CTabletTimeStamp` and logs
`CTL4100GraphicsTablet: PacketSize = %d`, `%s mode`.

**[IMPL]** The GUI shows X, Y, curved and raw pressure, distance, tilt,
in-range, tip, barrel 1/2, eraser, the 4-key bitmask, pps and monitors.

---

## 5. Rust reimplementation

Cargo workspace with four crates:

| Crate | Contents |
| --- | --- |
| `wacom-proto` | zero-allocation HID parser, feature reports, Bézier + LUT, affine transform, config, IPC |
| `wacomd` | hidapi → uinput, Hyprland IPC, Unix socket server, profiles, actions |
| `wacomctl` | clap + IPC client |
| `wacom-gui` | eframe/egui on Wayland |

Decisions:

1. **Structured Report `0x10`**, not the `0xAC` stream (see §2.8).
2. **uinput with axes normalized to the layout** (`0…32767`) instead of native
   units: this allows mapping to a monitor/region without depending on
   Hyprland's `input:tablet:output` setting. The daemon requires that option to
   be empty (mapping across the whole layout). The axes declare resolution
   216/345 so libinput sees 152 × 95 mm (see §7).
3. **Two virtual devices**: stylus (`ABS_X/Y/PRESSURE`, `BTN_TOOL_PEN`,
   `BTN_TOUCH`, `BTN_STYLUS/2`) and a macro keyboard/mouse (keys, buttons,
   `REL_WHEEL/HWHEEL`). No `BTN_TOOL_RUBBER`: the LP-1100K has no eraser.
4. **No `INPUT_PROP_DIRECT`**: the CTL-4100 is not a screen; `DIRECT` would
   make libinput treat it as a touchscreen and break mapping.
5. **Per-app profile** listening on `.socket2.sock`
   (`activewindow>>class,title`) and reading `hyprctl monitors -j` every 5 s.
6. **Passive init on Linux**: the kernel `wacom` driver already sends `02 02`
   and keeps Wacom mode. The daemon does not resend feature reports (resending
   `0D 00` paused the stream on the real CTL-4100); it only reads hidraw.
7. **Internal actions** (precision, monitor switch, radial) stay in the daemon;
   keys/clicks/shell go to the macro device.
8. **Unique virtual identity** (`1209:0001`, `CTL-4100 Userspace Pen`):
   Hyprland deduplicated a virtual stylus with the kernel's VID/PID/name and
   ignored its events. The kernel grab is done by name (`Wacom Intuos S
   Pen/Pad`) and does not affect the virtual device.
9. **GUI with asynchronous IPC**: saving configuration, pinning a profile and
   triggering actions do not block the egui thread; Hyprland no longer shows
   "Application Not Responding" when the daemon restarts.

---

## 6. Arch / Omarchy integration

- **udev**: keeps the kernel `wacom` driver bound (hidraw depends on it),
  grants access to the tablet's hidraw and evdev events via `uaccess` and
  allows uinput. The daemon issues `EVIOCGRAB` on the kernel events to avoid
  duplicate events.
- **systemd --user**: `wacomd.service` in `graphical-session.target`.
- **module**: `/etc/modules-load.d/wacom-ctl4100.conf` with `uinput`.
- **Hyprland**: monitors via `hyprctl monitors -j`; focus via socket2;
  `active_area`/rotation are done in the daemon, not in the compositor.
- **PKGBUILD**: builds the workspace, installs binaries, rule, unit, desktop
  file, license and documentation.

---

## 7. Limits, uncertainties and what was not verified

1. **Hardware tested on 2026-11-16.** CTL-4100 USB `056a:0374`, serial
   `0KH00R2105402`. The kernel `wacom` driver created `hidraw5` + event21/22;
   the daemon opened hidraw and issued `EVIOCGRAB` on both evdev nodes. 1,496
   27-byte reports were captured. The test validated pointer, pressure, side
   buttons and the 4 ExpressKeys. Fixes that came from the test:
   - in-range is bit `0x40` (`0x20` is vendor state);
   - there is no eraser bit in LP-1100K reports;
   - libinput ignores tablets with zero resolution on X/Y (216/345);
   - unbinding the kernel `wacom` driver removes hidraw; the solution is to
     keep the driver bound and use `EVIOCGRAB`;
   - the virtual stylus needs a unique name/VID/PID so Hyprland does not
     deduplicate it with the kernel tablet;
   - `wacom-gui` needed asynchronous IPC so the UI would not freeze.
2. **Pressure presets**: Bézier + LUT architecture confirmed; exact
   Soft/Firm preset points not recovered.
3. **Double click**: default distance/time are ours (10 px / 400 ms).
4. **Tilt**: the official driver's OAK path does not use tilt; our parser
   reads the fields but does not emit them on uinput.
5. **Eraser**: the LP-1100K has no eraser and the captured reports have no
   inversion bit; bit `0x36` is in-range. Different firmware may change this.
6. **Bluetooth**: BT mode uses 361-byte packets and feature `0x82`; not
   implemented. The project covers USB.
7. **`0xAC` raw**: observed, not implemented.
8. **Mapping**: the math is ours; the official driver uses `SMatrix` and
   "inset tablet mode" options that were not reproduced bit for bit.
9. **Hyprland `input:tablet:output`**: if set, the compositor rescales the
   device and the daemon's mapping becomes relative. Leave it empty.
10. **Performance**: the daemon reads with a 5 ms timeout and publishes
    telemetry at 20 Hz. In the captures the tablet delivered ~135 reports/s;
    the rate is limited by firmware.
11. **Radial menu**: it is an overlay inside the `wacom-gui` window (with
    forced focus), not an independent Wayland layer.
12. **Kernel**: if an update changes `hidraw`/`wacom` behavior, the udev rule
    and the grab may need review. The fallback is to stop `wacomd`: the grabs
    are released and the kernel driver serves the tablet again.

---

## 8. Reproducible evidence

Commands used (from `re-workspace/`, ignored by Git):

```bash
7z x WacomTablet_6.4.15-2.dmg
xar -xf InstallWacomTablet.pkg
gunzip -c content.pkg/Payload > payload.cpio
cpio -idm --no-absolute-filenames < payload.cpio

llvm-lipo WacomTabletDriver -thin x86_64 -output WacomTabletDriver.x86_64
llvm-nm --arch=x86_64 -C --defined-only WacomTabletDriver.x86_64 | wc -l
llvm-objdump -d --demangle --start-address=0x1002e0416 \
  --stop-address=0x1002e04c0 WacomTabletDriver.x86_64   # SetToWacomMode

/opt/ghidra/support/analyzeHeadless /tmp/opencode/ghidra_wacom WacomProj \
  -import WacomTabletDriver.x86_64 -scriptPath ghidra_scripts \
  -postScript DumpDecomp.java out.c CCTL4100 CGD16 CPressureCurve
```

Symbols/addresses cited (x86_64 slice):

| Address | Symbol |
| ---: | --- |
| `0x1000DC15C` | `CCTL4100Pen::Init` |
| `0x1000DC39A` | `CCTL4100Pen::HandleTransducerData` |
| `0x1000DFC62` | `CCTL4100GraphicsTablet::Init` |
| `0x1000DFEBE` | `CCTL4100GraphicsTablet::DeviceStart` |
| `0x1000E02C2` | `CCTL4100GraphicsTablet::ProcessDataPacket` |
| `0x1002E0364` | `MacHidDevice::SetToHIDMode` |
| `0x1002E0416` | `MacHidDevice::SetToWacomMode` |
| `0x100397C9E` | `CUSBCommInterface::SetToHIDMode` |
| `0x100399302` | `CUSBCommInterface::GD16SetScanModePen` |
| `0x10039939C` | `CUSBCommInterface::GD16SetScanModeTouch` |
| `0x100399DDE` | `CUSBCommInterface::GD16SendHardwareReset` |
| `0x1001AABF6` | `CPressureCurve::CPressureCurve` |
| `0x1001AADD2` | `CPressureCurve::EvaluateAtValue` |
| `0x1001AAE28` | `CPressureCurve::EvaluateAll` |
| `0x10023C642` | `CGD16StylusTransducer::HandleTransducerData` |
| `0x1003E80DE` | `CCTL4100TabletControls::ProcessCurrentTagList` |
| `0x10040CFD9` | `HidReport::TranslateShortEMR` |
| `0x10003731EE` | `CTransducer::CheckDoubleClickAssist` |
| `0x100310E58` | `CMappingOutputScreen::GetMonitorArea` |

The HID descriptor and auxiliary dumps stay in `re-workspace/kernel-ref/` and
`/tmp/opencode/wacom-hid-desc/` (outside Git). The working copy of
`wacom-hid-descriptors` is GPL-2.0; only the descriptor facts are cited here,
not the file.
