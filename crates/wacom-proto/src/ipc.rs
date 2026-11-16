//! Local IPC between `wacomd`, `wacomctl` and `wacom-gui`.
//!
//! Transport: Unix domain socket, one JSON object per line.

use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::config::{Action, Config};
use crate::transform::PixelRect;

pub const PROTOCOL_VERSION: u32 = 1;

pub fn socket_path() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        PathBuf::from(dir).join("wacom-ctl4100.sock")
    } else {
        // Fallback for a bare tty session without a runtime dir.
        let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
        std::env::temp_dir().join(format!("wacom-ctl4100-{user}.sock"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DeviceMode {
    Wacom,
    Hid,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Monitor {
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
    pub focused: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Telemetry {
    pub x: u32,
    pub y: u32,
    /// Curved pressure, 0..=4095.
    pub pressure: u16,
    /// Raw pressure from the report, 0..=4095.
    pub raw_pressure: u16,
    pub tip: bool,
    pub barrel: bool,
    pub barrel2: bool,
    pub eraser: bool,
    pub in_range: bool,
    /// Bit 0..=3 = ExpressKeys A..D.
    pub keys: u8,
    pub pps: f32,
    pub tilt_x: i32,
    pub tilt_y: i32,
    pub distance: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub connected: bool,
    pub product: String,
    pub serial: Option<String>,
    pub mode: DeviceMode,
    pub app_class: Option<String>,
    pub active_profile: Option<String>,
    #[serde(default)]
    pub pinned: bool,
    pub monitors: Vec<Monitor>,
    pub layout: PixelRect,
    pub telemetry: Option<Telemetry>,
    pub config_path: String,
}

impl Status {
    pub fn disconnected(config_path: String) -> Self {
        Self {
            connected: false,
            product: "Wacom Intuos S (CTL-4100)".into(),
            serial: None,
            mode: DeviceMode::Unknown,
            app_class: None,
            active_profile: None,
            pinned: false,
            monitors: Vec::new(),
            layout: PixelRect::new(0.0, 0.0, 1.0, 1.0),
            telemetry: None,
            config_path,
        }
    }
}

/// Bounding box of all monitors, in layout coordinates.
pub fn layout_of(monitors: &[Monitor]) -> PixelRect {
    let Some(first) = monitors.first() else {
        return PixelRect::new(0.0, 0.0, 1.0, 1.0);
    };
    let (mut x0, mut y0) = (first.x, first.y);
    let (mut x1, mut y1) = (first.x + first.width, first.y + first.height);
    for m in &monitors[1..] {
        x0 = x0.min(m.x);
        y0 = y0.min(m.y);
        x1 = x1.max(m.x + m.width);
        y1 = y1.max(m.y + m.height);
    }
    PixelRect::new(x0, y0, (x1 - x0).max(1.0), (y1 - y0).max(1.0))
}

#[allow(clippy::large_enum_variant)] // One Config per command; boxing is not worth the churn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "data", rename_all = "snake_case")]
pub enum Request {
    Status,
    GetConfig,
    SetConfig(Config),
    Subscribe,
    RunAction(Action),
    /// Pin the effective profile to an app class, or `None` to follow focus.
    PinProfile(Option<String>),
    Ping,
}

#[allow(clippy::large_enum_variant)] // Status/Config travel rarely; Telemetry is small.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum Event {
    Hello { version: u32 },
    Status(Status),
    Config(Config),
    Telemetry(Telemetry),
    Pad { index: u8, pressed: bool },
    Radial,
    Ack,
    Pong,
    Error { message: String },
}

pub fn write_message<W: Write, T: Serialize>(w: &mut W, value: &T) -> io::Result<()> {
    serde_json::to_writer(&mut *w, value).map_err(io_err)?;
    w.write_all(b"\n")?;
    w.flush()
}

pub fn read_message<R: BufRead, T: DeserializeOwned>(r: &mut R) -> io::Result<Option<T>> {
    let mut line = String::new();
    if r.read_line(&mut line)? == 0 {
        return Ok(None);
    }
    let value = serde_json::from_str(line.trim()).map_err(io_err)?;
    Ok(Some(value))
}

fn io_err(e: serde_json::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_wire_format() {
        assert_eq!(
            serde_json::to_string(&Request::Status).unwrap(),
            r#"{"cmd":"status"}"#
        );
        assert_eq!(
            serde_json::to_string(&Request::Subscribe).unwrap(),
            r#"{"cmd":"subscribe"}"#
        );
        let req = Request::RunAction(Action::Precision);
        assert_eq!(
            req,
            serde_json::from_str(&serde_json::to_string(&req).unwrap()).unwrap()
        );
    }

    #[test]
    fn event_round_trip() {
        let events = [
            Event::Hello { version: 1 },
            Event::Telemetry(Telemetry::default()),
            Event::Pad {
                index: 2,
                pressed: true,
            },
            Event::Radial,
        ];
        for e in events {
            let line = serde_json::to_string(&e).unwrap();
            let back: Event = serde_json::from_str(&line).unwrap();
            assert_eq!(e, back);
        }
    }

    #[test]
    fn layout_is_bounding_box() {
        let monitors = vec![
            Monitor {
                name: "eDP-1".into(),
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
                scale: 1.0,
                focused: true,
            },
            Monitor {
                name: "DP-1".into(),
                x: 1920.0,
                y: 0.0,
                width: 2560.0,
                height: 1440.0,
                scale: 1.0,
                focused: false,
            },
        ];
        assert_eq!(
            layout_of(&monitors),
            PixelRect::new(0.0, 0.0, 4480.0, 1440.0)
        );
    }

    #[test]
    fn empty_layout_is_sane() {
        assert_eq!(layout_of(&[]), PixelRect::new(0.0, 0.0, 1.0, 1.0));
    }
}
