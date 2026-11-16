//! Pure-Rust protocol model for the Wacom Intuos S (CTL-4100).
//!
//! - [`hid`]: input report parsing and vendor feature reports.
//! - [`pressure`]: Bezier pressure curves with a 32-sample lookup table.
//! - [`transform`]: tablet -> screen mapping (rotation, area, proportions).
//! - [`config`]: persistent configuration and per-application profiles.
//! - [`ipc`]: wire types shared by `wacomd`, `wacomctl` and `wacom-gui`.

pub mod config;
pub mod hid;
pub mod ipc;
pub mod pressure;
pub mod transform;

pub use config::{Action, Config, EffectiveConfig, MappingConfig, MouseAction, PenConfig, Profile};
pub use hid::{
    parse_input, BatteryReport, InputReport, PadReport, PenReport, PID_INTUOS_S,
    PID_INTUOS_S_WL_BT, PID_INTUOS_S_WL_BT_ALT, PID_INTUOS_S_WL_USB, PID_INTUOS_S_WL_USB_ALT,
    PRESSURE_MAX, SUPPORTED_PIDS, TABLET_HEIGHT, TABLET_WIDTH, USB_PIDS, VENDOR_ID,
};
pub use ipc::{DeviceMode, Event, Monitor, Request, Status, Telemetry};
pub use pressure::{CurveLut, CurvePoints, PressureCurve};
pub use transform::{Mapping, NormRect, Orientation, PixelRect};
