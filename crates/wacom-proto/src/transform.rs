//! Tablet -> screen affine mapping with rotation, active area and
//! "force proportions" fitting.
//!
//! The macOS driver models this as `CMappingInputTablet` (active area,
//! aspect type) plus `CMappingOutputScreen` (monitor selection), see
//! `docs/RE_REPORT_CTL4100.md` §3.2.

use serde::{Deserialize, Serialize};

use crate::hid::{TABLET_HEIGHT, TABLET_WIDTH};

/// uinput absolute axis range used by the virtual stylus.
pub const UINPUT_ABS_MAX: f64 = 32_767.0;

/// Physical tablet orientation, in degrees clockwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Orientation {
    #[default]
    #[serde(rename = "0")]
    Deg0,
    #[serde(rename = "90")]
    Deg90,
    #[serde(rename = "180")]
    Deg180,
    #[serde(rename = "270")]
    Deg270,
}

impl Orientation {
    pub fn degrees(self) -> u16 {
        match self {
            Orientation::Deg0 => 0,
            Orientation::Deg90 => 90,
            Orientation::Deg180 => 180,
            Orientation::Deg270 => 270,
        }
    }

    pub fn from_degrees(deg: u16) -> Option<Self> {
        Some(match deg {
            0 => Orientation::Deg0,
            90 => Orientation::Deg90,
            180 => Orientation::Deg180,
            270 => Orientation::Deg270,
            _ => return None,
        })
    }

    fn rotate(self, x: f64, y: f64) -> (f64, f64) {
        match self {
            Orientation::Deg0 => (x, y),
            Orientation::Deg90 => (1.0 - y, x),
            Orientation::Deg180 => (1.0 - x, 1.0 - y),
            Orientation::Deg270 => (y, 1.0 - x),
        }
    }

    fn swaps_axes(self) -> bool {
        matches!(self, Orientation::Deg90 | Orientation::Deg270)
    }
}

/// Rectangle in layout pixels (may start at a negative coordinate).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PixelRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl PixelRect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains(&self, p: (f64, f64)) -> bool {
        p.0 >= self.x && p.1 >= self.y && p.0 <= self.x + self.w && p.1 <= self.y + self.h
    }
}

/// Normalized sub-rectangle of the tablet active area (0..=1).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NormRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl NormRect {
    pub const FULL: NormRect = NormRect {
        x: 0.0,
        y: 0.0,
        w: 1.0,
        h: 1.0,
    };

    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    /// Sanitized copy: positive size, entirely inside the tablet.
    pub fn sanitized(self) -> Self {
        let w = self.w.clamp(0.01, 1.0);
        let h = self.h.clamp(0.01, 1.0);
        NormRect {
            x: self.x.clamp(0.0, 1.0 - w),
            y: self.y.clamp(0.0, 1.0 - h),
            w,
            h,
        }
    }
}

impl Default for NormRect {
    fn default() -> Self {
        Self::FULL
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mapping {
    pub tablet_area: NormRect,
    pub target: PixelRect,
    pub layout: PixelRect,
    pub orientation: Orientation,
    pub force_proportions: bool,
}

impl Mapping {
    /// Map native tablet coordinates to layout pixels.
    pub fn map(&self, x: f64, y: f64) -> (f64, f64) {
        let area = self.tablet_area.sanitized();
        let tw = f64::from(TABLET_WIDTH) * area.w;
        let th = f64::from(TABLET_HEIGHT) * area.h;

        let u = ((x - area.x * f64::from(TABLET_WIDTH)) / tw).clamp(0.0, 1.0);
        let v = ((y - area.y * f64::from(TABLET_HEIGHT)) / th).clamp(0.0, 1.0);

        let (rx, ry) = self.orientation.rotate(u, v);
        let (mut aspect, mut fit_w, mut fit_h) = (tw / th, self.target.w, self.target.h);
        if self.orientation.swaps_axes() {
            aspect = 1.0 / aspect;
        }
        if self.force_proportions {
            if self.target.w / self.target.h > aspect {
                fit_h = self.target.h;
                fit_w = fit_h * aspect;
            } else {
                fit_w = self.target.w;
                fit_h = fit_w / aspect;
            }
        }
        let ox = self.target.x + (self.target.w - fit_w) / 2.0;
        let oy = self.target.y + (self.target.h - fit_h) / 2.0;
        (ox + rx * fit_w, oy + ry * fit_h)
    }

    /// Four tablet corners in mapping order (TL, TR, BR, BL of the source area).
    pub fn corners(&self) -> [(f64, f64); 4] {
        let area = self.tablet_area.sanitized();
        let x0 = area.x * f64::from(TABLET_WIDTH);
        let y0 = area.y * f64::from(TABLET_HEIGHT);
        let x1 = (area.x + area.w) * f64::from(TABLET_WIDTH);
        let y1 = (area.y + area.h) * f64::from(TABLET_HEIGHT);
        [
            self.map(x0, y0),
            self.map(x1, y0),
            self.map(x1, y1),
            self.map(x0, y1),
        ]
    }
}

/// Convert layout pixels to the virtual device's 0..=32767 range.
pub fn to_uinput(p: (f64, f64), layout: PixelRect) -> (u16, u16) {
    let nx = if layout.w > 0.0 {
        (p.0 - layout.x) / layout.w
    } else {
        0.0
    };
    let ny = if layout.h > 0.0 {
        (p.1 - layout.y) / layout.h
    } else {
        0.0
    };
    (
        (nx.clamp(0.0, 1.0) * UINPUT_ABS_MAX).round() as u16,
        (ny.clamp(0.0, 1.0) * UINPUT_ABS_MAX).round() as u16,
    )
}

/// Precision mode: move only a fraction of the requested delta.
pub fn apply_precision(prev: (f64, f64), next: (f64, f64), factor: f64) -> (f64, f64) {
    (
        prev.0 + (next.0 - prev.0) * factor,
        prev.1 + (next.1 - prev.1) * factor,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> PixelRect {
        PixelRect::new(0.0, 0.0, 1920.0, 1080.0)
    }

    fn mapping(orientation: Orientation, force_proportions: bool) -> Mapping {
        Mapping {
            tablet_area: NormRect::FULL,
            target: screen(),
            layout: screen(),
            orientation,
            force_proportions,
        }
    }

    #[test]
    fn full_area_stretches_to_screen() {
        let m = mapping(Orientation::Deg0, false);
        assert_eq!(m.map(0.0, 0.0), (0.0, 0.0));
        assert_eq!(m.map(15200.0, 9500.0), (1920.0, 1080.0));
        assert_eq!(m.map(7600.0, 4750.0), (960.0, 540.0));
    }

    #[test]
    fn rotation_maps_corners_clockwise() {
        let m90 = mapping(Orientation::Deg90, false);
        assert_eq!(m90.map(0.0, 0.0), (1920.0, 0.0));
        assert_eq!(m90.map(15200.0, 0.0), (1920.0, 1080.0));

        let m180 = mapping(Orientation::Deg180, false);
        assert_eq!(m180.map(0.0, 0.0), (1920.0, 1080.0));

        let m270 = mapping(Orientation::Deg270, false);
        assert_eq!(m270.map(0.0, 0.0), (0.0, 1080.0));
        assert_eq!(m270.map(15200.0, 0.0), (0.0, 0.0));
    }

    #[test]
    fn force_proportions_letterboxes_and_centers() {
        let m = mapping(Orientation::Deg0, true);
        // Tablet aspect 1.6, screen 1.777 -> fit height, 1728 wide, 96 px margins.
        assert_eq!(m.map(0.0, 0.0), (96.0, 0.0));
        assert_eq!(m.map(15200.0, 9500.0), (1824.0, 1080.0));
        assert_eq!(m.map(7600.0, 4750.0), (960.0, 540.0));
    }

    #[test]
    fn rotated_force_proportions_uses_swapped_aspect() {
        let m = mapping(Orientation::Deg90, true);
        // Aspect 0.625 -> fit height, width 675, x offset 622.5.
        // The tablet's bottom-left corner rotates to the top-left of the screen.
        let (x, y) = m.map(0.0, 9500.0);
        assert!((x - 622.5).abs() < 1e-9, "x = {x}");
        assert_eq!(y, 0.0);
    }

    #[test]
    fn active_area_is_scaled_from_sub_rectangle() {
        let mut m = mapping(Orientation::Deg0, false);
        m.tablet_area = NormRect::new(0.5, 0.5, 0.5, 0.5);
        assert_eq!(m.map(7600.0, 4750.0), (0.0, 0.0));
        assert_eq!(m.map(15200.0, 9500.0), (1920.0, 1080.0));
    }

    #[test]
    fn normalizes_into_uinput_range() {
        assert_eq!(to_uinput((0.0, 0.0), screen()), (0, 0));
        assert_eq!(to_uinput((1920.0, 1080.0), screen()), (32767, 32767));
        assert_eq!(to_uinput((960.0, 540.0), screen()), (16384, 16384));
    }

    #[test]
    fn precision_halves_the_delta() {
        assert_eq!(
            apply_precision((100.0, 100.0), (200.0, 60.0), 0.5),
            (150.0, 80.0)
        );
    }

    #[test]
    fn offset_layout_is_supported() {
        let layout = PixelRect::new(-1920.0, 0.0, 3840.0, 1080.0);
        let m = Mapping {
            target: PixelRect::new(-1920.0, 0.0, 1920.0, 1080.0),
            layout,
            tablet_area: NormRect::FULL,
            orientation: Orientation::Deg0,
            force_proportions: false,
        };
        assert_eq!(m.map(0.0, 0.0), (-1920.0, 0.0));
        assert_eq!(m.map(15200.0, 9500.0), (0.0, 1080.0));
        assert_eq!(to_uinput((-1920.0, 0.0), layout), (0, 0));
        assert_eq!(to_uinput((0.0, 1080.0), layout), (16384, 32767));
    }
}
