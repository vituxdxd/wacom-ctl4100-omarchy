//! CTL-4100 HID protocol: input report parsing and vendor feature reports.
//!
//! Wire format source: report descriptor of a real CTL-4100
//! (`056a:0374`, HID v1.10), see `docs/RE_REPORT_CTL4100.md` §2.

use thiserror::Error;

pub const VENDOR_ID: u16 = 0x056a;

/// CTL-4100 (Intuos S, wired).
pub const PID_INTUOS_S: u16 = 0x0374;
/// CTL-4100WL over USB cable.
pub const PID_INTUOS_S_WL_USB: u16 = 0x0376;
/// CTL-4100WL over Bluetooth.
pub const PID_INTUOS_S_WL_BT: u16 = 0x0377;
/// CTL-4100WL (2022 refresh) over USB.
pub const PID_INTUOS_S_WL_USB_ALT: u16 = 0x03c5;
/// CTL-4100WL (2022 refresh) over Bluetooth.
pub const PID_INTUOS_S_WL_BT_ALT: u16 = 0x03c6;

pub const SUPPORTED_PIDS: [u16; 5] = [
    PID_INTUOS_S,
    PID_INTUOS_S_WL_USB,
    PID_INTUOS_S_WL_BT,
    PID_INTUOS_S_WL_USB_ALT,
    PID_INTUOS_S_WL_BT_ALT,
];

/// PIDs the userspace driver can actually open. Bluetooth transports use a
/// different packet size and feature set and are left to the kernel.
pub const USB_PIDS: [u16; 3] = [PID_INTUOS_S, PID_INTUOS_S_WL_USB, PID_INTUOS_S_WL_USB_ALT];

/// Active area in device units (100 units/mm, 2540 lpi).
pub const TABLET_WIDTH: u32 = 15_200;
pub const TABLET_HEIGHT: u32 = 9_500;
pub const PRESSURE_MAX: u16 = 4_095;
/// Distance (Z) maximum from the descriptor, used for hover telemetry.
pub const DISTANCE_MAX: u8 = 63;

pub const REPORT_PEN: u8 = 0x10;
pub const REPORT_PAD: u8 = 0x11;
pub const REPORT_BATTERY: u8 = 0x13;
/// Vendor raw stream (used by the macOS driver, not by this one).
pub const REPORT_RAW: u8 = 0xac;

/// Size of a pen report including the report ID.
pub const PEN_REPORT_LEN: usize = 27;

/// Bit positions inside the pen status byte (payload byte 0), verified against
/// captured CTL-4100 reports (see `docs/RE_REPORT_CTL4100.md` §2.3).
pub const PEN_TIP: u8 = 1 << 0; // usage 0x42 TipSwitch
pub const PEN_BARREL: u8 = 1 << 1; // usage 0x44 BarrelSwitch -> BTN_STYLUS
pub const PEN_BARREL2: u8 = 1 << 2; // usage 0x5A BarrelSwitch2 -> BTN_STYLUS2
/// Vendor state set during hover and tip contact (usage 0x32 in the descriptor).
pub const PEN_AUX: u8 = 1 << 5;
/// Set while the pen is inside the active area (usage 0x36 in the descriptor).
pub const PEN_IN_RANGE: u8 = 1 << 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PenReport {
    pub status: u8,
    pub x: u32,
    pub y: u32,
    pub pressure: u16,
    pub distance: u8,
    pub tilt_x: i32,
    pub tilt_y: i32,
    pub twist: u16,
}

impl PenReport {
    pub fn tip(&self) -> bool {
        self.status & PEN_TIP != 0
    }
    pub fn barrel(&self) -> bool {
        self.status & PEN_BARREL != 0
    }
    pub fn barrel2(&self) -> bool {
        self.status & PEN_BARREL2 != 0
    }
    /// The CTL-4100 ships with the LP-1100K, which has no eraser. Captured
    /// reports never carry an invert bit in this status byte.
    pub fn eraser(&self) -> bool {
        false
    }
    pub fn in_range(&self) -> bool {
        self.status & PEN_IN_RANGE != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PadReport {
    /// Raw byte holding ExpressKey bits 0..=3.
    pub keys: u8,
}

impl PadReport {
    /// `index` is 0-based, matching hardware order A..D (left to right).
    pub fn key(&self, index: u8) -> bool {
        index < 4 && self.keys & (1 << index) != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatteryReport {
    pub level: u8,
    pub charging: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputReport {
    Pen(PenReport),
    Pad(PadReport),
    Battery(BatteryReport),
    /// Report IDs we do not decode (raw vendor stream, heartbeat, ...).
    Other {
        report_id: u8,
        len: usize,
    },
}

impl InputReport {
    pub fn report_id(&self) -> u8 {
        match self {
            InputReport::Pen(_) => REPORT_PEN,
            InputReport::Pad(_) => REPORT_PAD,
            InputReport::Battery(_) => REPORT_BATTERY,
            InputReport::Other { report_id, .. } => *report_id,
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("empty report")]
    Empty,
    #[error("truncated report 0x{report_id:02x}: got {got} bytes, need {need}")]
    Truncated {
        report_id: u8,
        got: usize,
        need: usize,
    },
}

/// Parse one HID input report. `buf[0]` is the report ID, as returned by hidraw.
pub fn parse_input(buf: &[u8]) -> Result<InputReport, ParseError> {
    let (&id, payload) = buf.split_first().ok_or(ParseError::Empty)?;
    match id {
        REPORT_PEN => {
            if buf.len() < PEN_REPORT_LEN {
                return Err(ParseError::Truncated {
                    report_id: id,
                    got: buf.len(),
                    need: PEN_REPORT_LEN,
                });
            }
            Ok(InputReport::Pen(PenReport {
                status: payload[0],
                // 24-bit little-endian, logical range 0..=15200 / 0..=9500.
                x: u24(&payload[1..4]),
                y: u24(&payload[4..7]),
                pressure: u16::from_le_bytes([payload[7], payload[8]]),
                // payload[9..15] is constant padding.
                distance: payload[15],
                tilt_x: i32::from_le_bytes([payload[16], payload[17], payload[18], payload[19]]),
                tilt_y: i32::from_le_bytes([payload[20], payload[21], payload[22], payload[23]]),
                twist: u16::from_le_bytes([payload[24], payload[25]]),
            }))
        }
        REPORT_PAD => {
            if payload.is_empty() {
                return Err(ParseError::Truncated {
                    report_id: id,
                    got: buf.len(),
                    need: 2,
                });
            }
            Ok(InputReport::Pad(PadReport { keys: payload[0] }))
        }
        REPORT_BATTERY => {
            if payload.is_empty() {
                return Err(ParseError::Truncated {
                    report_id: id,
                    got: buf.len(),
                    need: 2,
                });
            }
            Ok(InputReport::Battery(BatteryReport {
                level: payload[0] & 0x7f,
                charging: payload[0] & 0x80 != 0,
            }))
        }
        _ => Ok(InputReport::Other {
            report_id: id,
            len: buf.len(),
        }),
    }
}

fn u24(b: &[u8]) -> u32 {
    u32::from(b[0]) | (u32::from(b[1]) << 8) | (u32::from(b[2]) << 16)
}

/// Feature report `0x02 = 0x02`: leave mouse/HID mode, enable Wacom report mode.
///
/// `MacHidDevice::SetToWacomMode()` at `0x1002e0416` builds exactly these two
/// bytes and sends them with `IOHIDDeviceSetReport(kIOHIDReportTypeFeature)`.
pub const FEATURE_WACOM_MODE: [u8; 2] = [0x02, 0x02];
/// Feature report `0x02 = 0x01`: return to generic HID/mouse mode.
pub const FEATURE_HID_MODE: [u8; 2] = [0x02, 0x01];
/// Feature report `0x0d`: pen scan enable (`0`) / disable (`1`).
pub fn feature_pen_scan(enabled: bool) -> [u8; 2] {
    [0x0d, u8::from(!enabled)]
}
pub const FEATURE_HARDWARE_RESET: [u8; 3] = [0xcd, 0xa6, 0x50];

/// Feature report `0x14`: ask the tablet to emit its serial number report.
/// Only the first byte carries the command; the remaining bytes are zero.
pub const FEATURE_GET_SERIAL: [u8; 14] = [0x14, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

#[cfg(test)]
mod tests {
    use super::*;

    fn pen_buf() -> [u8; PEN_REPORT_LEN] {
        let mut b = [0u8; PEN_REPORT_LEN];
        b[0] = REPORT_PEN;
        b[1] = PEN_TIP | PEN_IN_RANGE | PEN_BARREL | PEN_BARREL2; // 0x27
                                                                  // X = 12345 = 0x3039, Y = 6789 = 0x1a85
        b[2] = 0x39;
        b[3] = 0x30;
        b[4] = 0x00;
        b[5] = 0x85;
        b[6] = 0x1a;
        b[8] = 0x00;
        b[9] = 0x08; // 2048
        b[16] = 7; // distance
        b[17..21].copy_from_slice(&(-1234i32).to_le_bytes());
        b[21..25].copy_from_slice(&(5678i32).to_le_bytes());
        b[25] = 0x10;
        b[26] = 0x20;
        b
    }

    #[test]
    fn parses_pen_report() {
        let r = match parse_input(&pen_buf()).unwrap() {
            InputReport::Pen(p) => p,
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(r.x, 12345);
        assert_eq!(r.y, 6789);
        assert_eq!(r.pressure, 2048);
        assert_eq!(r.distance, 7);
        assert_eq!((r.tilt_x, r.tilt_y), (-1234, 5678));
        assert_eq!(r.twist, 0x2010);
        assert!(r.tip() && r.barrel() && r.barrel2() && r.in_range());
        assert!(!r.eraser());
    }

    #[test]
    fn parses_pad_bits() {
        let r = match parse_input(&[REPORT_PAD, 0b1010, 0, 0, 0, 0, 0, 0, 0]).unwrap() {
            InputReport::Pad(p) => p,
            other => panic!("unexpected {other:?}"),
        };
        assert!(!r.key(0) && r.key(1) && !r.key(2) && r.key(3));
        assert_eq!(r, PadReport { keys: 0b1010 });
    }

    #[test]
    fn parses_battery() {
        let r = match parse_input(&[REPORT_BATTERY, 0x80 | 55]).unwrap() {
            InputReport::Battery(b) => b,
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!((r.level, r.charging), (55, true));
    }

    #[test]
    fn decodes_captured_hover_report() {
        // Real CTL-4100 hover: status 0x40, X=0x1b93, Y=0x0d5b, no pressure.
        let buf = [
            0x10, 0x40, 0x93, 0x1b, 0x00, 0x5b, 0x0d, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x3f, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let r = match parse_input(&buf).unwrap() {
            InputReport::Pen(p) => p,
            other => panic!("unexpected {other:?}"),
        };
        assert!(r.in_range());
        assert!(!r.tip());
        assert!(!r.barrel());
        assert!(!r.eraser());
        assert_eq!((r.x, r.y), (0x1b93, 0x0d5b));
        assert_eq!(r.pressure, 0);
    }

    #[test]
    fn decodes_captured_tip_report() {
        // Real CTL-4100 tip contact: status 0x61, X=0x23e9, Y=0x18f9,
        // pressure 0x00ab = 171.
        let buf = [
            0x10, 0x61, 0xe9, 0x23, 0x00, 0xf9, 0x18, 0x00, 0xab, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x12, 0x12, 0x66, 0x83, 0x0b, 0x62, 0x08, 0x10, 0x00, 0x62, 0x08,
        ];
        let r = match parse_input(&buf).unwrap() {
            InputReport::Pen(p) => p,
            other => panic!("unexpected {other:?}"),
        };
        assert!(r.in_range());
        assert!(r.tip());
        assert_eq!((r.x, r.y), (0x23e9, 0x18f9));
        assert_eq!(r.pressure, 171);
    }

    #[test]
    fn rejects_truncated_pen() {
        let buf = pen_buf();
        assert_eq!(
            parse_input(&buf[..26]).unwrap_err(),
            ParseError::Truncated {
                report_id: REPORT_PEN,
                got: 26,
                need: PEN_REPORT_LEN
            }
        );
    }

    #[test]
    fn keeps_unknown_reports() {
        assert_eq!(
            parse_input(&[REPORT_RAW, 1, 2, 3]).unwrap(),
            InputReport::Other {
                report_id: REPORT_RAW,
                len: 4
            }
        );
    }

    #[test]
    fn feature_reports_match_observed_bytes() {
        assert_eq!(FEATURE_WACOM_MODE, [0x02, 0x02]);
        assert_eq!(FEATURE_HID_MODE, [0x02, 0x01]);
        assert_eq!(feature_pen_scan(true), [0x0d, 0x00]);
        assert_eq!(feature_pen_scan(false), [0x0d, 0x01]);
        assert_eq!(FEATURE_HARDWARE_RESET, [0xcd, 0xa6, 0x50]);
        assert_eq!(FEATURE_GET_SERIAL[0], 0x14);
    }
}
