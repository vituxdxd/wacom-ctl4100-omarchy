//! Pressure curve: cubic Bezier, sampled into a 32-entry lookup table.
//!
//! The macOS driver models pressure as `CPressureCurve : CBezierCurve` with
//! `EvaluateAll()` precomputing a lookup table (`CBezierCurve::CBezierCurve(type, 0x20)`),
//! see `docs/RE_REPORT_CTL4100.md` §3.1.

use serde::{Deserialize, Serialize};

use crate::hid::PRESSURE_MAX;

/// Two free control points of a cubic Bezier from (0,0) to (1,1).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CurvePoints {
    pub p1: [f32; 2],
    pub p2: [f32; 2],
}

impl CurvePoints {
    pub const LINEAR: CurvePoints = CurvePoints {
        p1: [1.0 / 3.0, 1.0 / 3.0],
        p2: [2.0 / 3.0, 2.0 / 3.0],
    };
    pub const SOFT: CurvePoints = CurvePoints {
        p1: [0.05, 0.55],
        p2: [0.35, 0.98],
    };
    pub const FIRM: CurvePoints = CurvePoints {
        p1: [0.62, 0.10],
        p2: [0.95, 0.45],
    };

    fn sanitized(self) -> Self {
        let clamp = |v: f32| v.clamp(0.0, 1.0);
        let p1 = [clamp(self.p1[0]), clamp(self.p1[1])];
        let p2 = [clamp(self.p2[0]), clamp(self.p2[1])];
        // Monotone x and y make the curve invertible and non-decreasing.
        CurvePoints {
            p1,
            p2: [p2[0].max(p1[0]), p2[1].max(p1[1])],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PressureCurve {
    points: CurvePoints,
}

impl PressureCurve {
    pub fn new(points: CurvePoints) -> Self {
        Self {
            points: points.sanitized(),
        }
    }

    pub fn linear() -> Self {
        Self::new(CurvePoints::LINEAR)
    }

    /// `feel` in `0.0..=1.0`: 0 = soft (low pressure reaches full output),
    /// 0.5 = linear, 1 = firm.
    pub fn from_tip_feel(feel: f32) -> Self {
        let feel = feel.clamp(0.0, 1.0);
        let lerp =
            |a: [f32; 2], b: [f32; 2], t: f32| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        let points = if feel < 0.5 {
            let t = feel * 2.0;
            CurvePoints {
                p1: lerp(CurvePoints::SOFT.p1, CurvePoints::LINEAR.p1, t),
                p2: lerp(CurvePoints::SOFT.p2, CurvePoints::LINEAR.p2, t),
            }
        } else {
            let t = (feel - 0.5) * 2.0;
            CurvePoints {
                p1: lerp(CurvePoints::LINEAR.p1, CurvePoints::FIRM.p1, t),
                p2: lerp(CurvePoints::LINEAR.p2, CurvePoints::FIRM.p2, t),
            }
        };
        Self::new(points)
    }

    pub fn points(&self) -> CurvePoints {
        self.points
    }

    /// Evaluate the curve for normalized input `x` in `0.0..=1.0`.
    pub fn eval(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        if x <= 0.0 {
            return 0.0;
        }
        if x >= 1.0 {
            return 1.0;
        }
        // Invert x(t) by bisection; x(t) is monotone after sanitizing.
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..24 {
            let mid = 0.5 * (lo + hi);
            if bezier(mid, 0.0, self.points.p1[0], self.points.p2[0], 1.0) < x {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let t = 0.5 * (lo + hi);
        bezier(t, 0.0, self.points.p1[1], self.points.p2[1], 1.0)
    }

    /// 33-entry table, matching the 32-sample cache used by the macOS driver.
    pub fn lut(&self) -> [u16; 33] {
        let mut out = [0u16; 33];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = (self.eval(i as f32 / 32.0) * f32::from(PRESSURE_MAX)).round() as u16;
        }
        out
    }
}

/// Lookup-table evaluator used by the daemon, O(1) per report.
#[derive(Debug, Clone)]
pub struct CurveLut {
    table: [u16; 33],
}

impl CurveLut {
    pub fn new(curve: &PressureCurve) -> Self {
        Self { table: curve.lut() }
    }

    pub fn eval(&self, raw: u16) -> u16 {
        let raw = raw.min(PRESSURE_MAX);
        let pos = f32::from(raw) * 32.0 / f32::from(PRESSURE_MAX);
        let i = (pos.floor() as usize).min(32);
        let frac = pos - i as f32;
        if i == 32 {
            self.table[32]
        } else {
            let a = f32::from(self.table[i]);
            let b = f32::from(self.table[i + 1]);
            (a + (b - a) * frac).round() as u16
        }
    }
}

fn bezier(t: f32, p0: f32, p1: f32, p2: f32, p3: f32) -> f32 {
    let u = 1.0 - t;
    u * u * u * p0 + 3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t * p3
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(a: f32, b: f32, eps: f32) {
        assert!((a - b).abs() <= eps, "{a} != {b} (eps {eps})");
    }

    #[test]
    fn linear_is_identity() {
        let c = PressureCurve::linear();
        for i in 0..=100 {
            let x = i as f32 / 100.0;
            assert_close(c.eval(x), x, 0.002);
        }
        let lut = CurveLut::new(&c);
        for raw in [0u16, 1, 1000, 2048, 4095] {
            let got = lut.eval(raw);
            assert!(got.abs_diff(raw) <= 2, "raw {raw}: got {got}");
        }
    }

    #[test]
    fn soft_reaches_more_output_than_firm() {
        let soft = PressureCurve::from_tip_feel(0.0);
        let firm = PressureCurve::from_tip_feel(1.0);
        assert!(soft.eval(0.25) > firm.eval(0.25));
        assert!(soft.eval(0.5) > firm.eval(0.5));
        assert_eq!(soft.eval(0.0), 0.0);
        assert_eq!(soft.eval(1.0), 1.0);
    }

    #[test]
    fn tip_feel_midpoint_is_linear() {
        let mid = PressureCurve::from_tip_feel(0.5);
        assert_eq!(mid.points(), CurvePoints::LINEAR);
    }

    #[test]
    fn curve_is_monotone() {
        for feel in [0.0, 0.2, 0.5, 0.8, 1.0] {
            let c = PressureCurve::from_tip_feel(feel);
            let mut prev = 0.0;
            for i in 0..=200 {
                let y = c.eval(i as f32 / 200.0);
                assert!(y + 1e-5 >= prev, "feel {feel}: dip at {i}");
                prev = y;
            }
        }
    }

    #[test]
    fn custom_points_are_clamped_and_ordered() {
        let c = PressureCurve::new(CurvePoints {
            p1: [2.0, -1.0],
            p2: [-1.0, 2.0],
        });
        let p = c.points();
        assert_eq!(p.p1, [1.0, 0.0]);
        assert_eq!(p.p2, [1.0, 1.0]);
        assert_eq!(c.eval(0.0), 0.0);
        assert_eq!(c.eval(1.0), 1.0);
    }

    #[test]
    fn lut_endpoints() {
        let lut = CurveLut::new(&PressureCurve::from_tip_feel(0.0));
        assert_eq!(lut.eval(0), 0);
        assert_eq!(lut.eval(PRESSURE_MAX), PRESSURE_MAX);
    }
}
