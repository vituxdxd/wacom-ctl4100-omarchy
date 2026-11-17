//! Device loop: hidraw reports -> virtual stylus/pad -> IPC telemetry.

use std::io;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use evdev::uinput::VirtualDevice;
use evdev::{
    AbsInfo, AbsoluteAxisCode, AttributeSet, BusType, EventType, InputEvent, InputId, KeyCode,
    UinputAbsSetup,
};
use hidapi::{HidApi, HidDevice};
use wacom_proto::config::{self, Action, Config, EffectiveConfig, MouseAction, ScreenSelection};
use wacom_proto::hid::{parse_input, InputReport, PenReport, PRESSURE_MAX, USB_PIDS, VENDOR_ID};
use wacom_proto::ipc::{layout_of, DeviceMode, Event, Monitor, Status, Telemetry};
use wacom_proto::transform::{apply_precision, to_uinput, Mapping, PixelRect};
use wacom_proto::CurveLut;

use crate::actions::{run_external, ActionRunner};
use crate::hyprland::{self, HyprEvent};
use crate::server::{Command, Shared};

const PRECISION_FACTOR: f64 = 0.5;
const SCROLL_STEP: f64 = 12.0;
const MAX_PEN_REPORT: usize = 64;

pub struct Driver {
    api: HidApi,
    device: Option<HidDevice>,
    product: String,
    serial: Option<String>,
    mode: DeviceMode,
    stylus: VirtualDevice,
    pointer: VirtualDevice,
    runner: ActionRunner,
    shared: Arc<Shared>,
    cmd_rx: Receiver<Command>,
    hypr_rx: Receiver<HyprEvent>,
    config_path: PathBuf,
    base: Config,
    effective: EffectiveConfig,
    app_class: Option<String>,
    pinned: bool,
    monitors: Vec<Monitor>,
    layout: PixelRect,
    mapping: Mapping,
    lut: CurveLut,
    telemetry: Telemetry,
    pen: PenState,
    pad_keys: u8,
    kernel_grabs: Vec<evdev::Device>,
    grabbed_paths: std::collections::HashSet<PathBuf>,
    last_grab_attempt: Instant,
    precision: bool,
    scroll_mode: bool,
    scroll_click: bool,
    wheel_accum: (f64, f64),
    last_scroll: Option<(f64, f64)>,
    last_emitted: Option<(f64, f64)>,
    last_down: Option<(Instant, (f64, f64))>,
    pps_count: u32,
    pps_start: Instant,
    last_publish: Instant,
    last_status_broadcast: Instant,
    last_monitor_poll: Instant,
    last_open_attempt: Instant,
}

#[derive(Debug, Clone, Copy, Default)]
struct PenState {
    in_range: bool,
    tip: bool,
    tool_pen: bool,
    tool_rubber: bool,
    /// Previous physical button state, for press-edge detection.
    barrel: bool,
    barrel2: bool,
    /// Currently emitted native BTN_STYLUS / BTN_STYLUS2.
    native_stylus: bool,
    native_stylus2: bool,
}

impl Driver {
    pub fn new(
        shared: Arc<Shared>,
        cmd_rx: Receiver<Command>,
        hypr_rx: Receiver<HyprEvent>,
        base: Config,
        config_path: PathBuf,
    ) -> io::Result<Self> {
        let api = HidApi::new().map_err(|e| io::Error::other(format!("hidapi: {e}")))?;
        let mut monitors = hyprland::monitors().unwrap_or_default();
        if monitors.is_empty() {
            monitors.push(Monitor {
                name: "default".into(),
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0,
                scale: 1.0,
                focused: true,
            });
        }
        let layout = layout_of(&monitors);
        let app_class = hyprland::active_class();
        let effective = base.effective(app_class.as_deref());
        let lut = CurveLut::new(&effective.pen.curve());
        let mapping = build_mapping(&effective, &monitors, layout);
        let stylus = build_stylus()?;
        let pointer = build_pointer()?;
        let runner = ActionRunner::new()?;
        Ok(Self {
            api,
            device: None,
            product: "Wacom Intuos S (CTL-4100)".into(),
            serial: None,
            mode: DeviceMode::Unknown,
            stylus,
            pointer,
            runner,
            shared,
            cmd_rx,
            hypr_rx,
            config_path,
            base,
            effective,
            app_class,
            pinned: false,
            monitors,
            layout,
            mapping,
            lut,
            telemetry: Telemetry::default(),
            pen: PenState::default(),
            pad_keys: 0,
            kernel_grabs: Vec::new(),
            grabbed_paths: std::collections::HashSet::new(),
            last_grab_attempt: Instant::now() - Duration::from_secs(5),
            precision: false,
            scroll_mode: false,
            scroll_click: false,
            wheel_accum: (0.0, 0.0),
            last_scroll: None,
            last_emitted: None,
            last_down: None,
            pps_count: 0,
            pps_start: Instant::now(),
            last_publish: Instant::now(),
            last_status_broadcast: Instant::now() - Duration::from_secs(2),
            last_monitor_poll: Instant::now(),
            last_open_attempt: Instant::now() - Duration::from_secs(5),
        })
    }

    pub fn run(&mut self) {
        let mut buf = [0u8; MAX_PEN_REPORT];
        loop {
            self.drain_commands();
            self.drain_hyprland();

            if self.device.is_none() && self.last_open_attempt.elapsed() >= Duration::from_secs(1) {
                self.try_open();
            }
            if self.last_grab_attempt.elapsed() >= Duration::from_secs(1) {
                self.grab_kernel_inputs();
                self.last_grab_attempt = Instant::now();
            }

            if let Some(device) = &self.device {
                match device.read_timeout(&mut buf, 5) {
                    Ok(0) => {}
                    Ok(n) => self.handle_report(&buf[..n]),
                    Err(_) => self.disconnect(),
                }
            } else {
                thread::sleep(Duration::from_millis(5));
            }

            if self.last_monitor_poll.elapsed() >= Duration::from_secs(5) {
                self.refresh_monitors();
            }
            if self.last_publish.elapsed() >= Duration::from_millis(50) {
                self.publish();
                self.last_publish = Instant::now();
            }
            self.update_pps();
        }
    }

    fn try_open(&mut self) {
        self.last_open_attempt = Instant::now();
        let found = self
            .api
            .device_list()
            .find(|d| d.vendor_id() == VENDOR_ID && USB_PIDS.contains(&d.product_id()))
            .cloned();
        let Some(info) = found else {
            return;
        };
        let device = match info.open_device(&self.api) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("wacomd: open 056a:{:04x} failed: {e}", info.product_id());
                return;
            }
        };
        self.product = device
            .get_product_string()
            .ok()
            .flatten()
            .unwrap_or_else(|| "Wacom Intuos S (CTL-4100)".into());
        self.serial = device.get_serial_number_string().ok().flatten();
        // The in-kernel wacom driver owns mode initialization. Sending the
        // same feature reports again (notably 0x0d) can pause the report
        // stream, so wacomd is deliberately passive on hidraw.
        self.mode = DeviceMode::Wacom;
        eprintln!(
            "wacomd: connected {} (056a:{:04x})",
            self.product,
            info.product_id()
        );
        self.device = Some(device);
        self.grab_kernel_inputs();
        self.publish();
    }

    /// Keep the kernel `wacom` driver bound (it owns the hidraw node) but take
    /// its evdev nodes exclusively so pen/pad events are not duplicated.
    fn grab_kernel_inputs(&mut self) {
        for (path, mut device) in evdev::enumerate() {
            let id = device.input_id();
            if id.vendor() != VENDOR_ID || !USB_PIDS.contains(&id.product()) {
                continue;
            }
            let name = device.name().unwrap_or_default().to_string();
            if name != "Wacom Intuos S Pen" && name != "Wacom Intuos S Pad" {
                continue;
            }
            if self.grabbed_paths.contains(&path) {
                continue;
            }
            match device.grab() {
                Ok(()) => {
                    eprintln!("wacomd: grabbed kernel input {name} ({})", path.display());
                    self.grabbed_paths.insert(path);
                    self.kernel_grabs.push(device);
                }
                Err(e) => eprintln!("wacomd: cannot grab {name} ({}): {e}", path.display()),
            }
        }
    }

    fn disconnect(&mut self) {
        self.release_all();
        self.kernel_grabs.clear();
        self.grabbed_paths.clear();
        self.device = None;
        self.mode = DeviceMode::Unknown;
        self.pen = PenState::default();
        eprintln!("wacomd: device disconnected");
        self.publish();
    }

    fn drain_commands(&mut self) {
        while let Ok(cmd) = self.cmd_rx.try_recv() {
            match cmd {
                Command::Config(cfg) => {
                    self.base = cfg;
                    self.apply_effective();
                }
                Command::Action(action) => self.run_action(action),
                Command::Pin(app) => {
                    self.pinned = app.is_some();
                    self.app_class = app;
                    self.apply_effective();
                }
            }
        }
    }

    fn drain_hyprland(&mut self) {
        while let Ok(ev) = self.hypr_rx.try_recv() {
            match ev {
                HyprEvent::AppChanged(app) if !self.pinned => {
                    if app != self.app_class {
                        self.app_class = app;
                        self.apply_effective();
                    }
                }
                HyprEvent::AppChanged(_) => {}
                HyprEvent::MonitorsChanged => self.refresh_monitors(),
            }
        }
    }

    fn apply_effective(&mut self) {
        // Release anything held with the old mode before switching devices.
        self.leave_proximity();
        self.effective = self.base.effective(self.app_class.as_deref());
        self.mapping = build_mapping(&self.effective, &self.monitors, self.layout);
        self.lut = CurveLut::new(&self.effective.pen.curve());
        self.last_emitted = None;
        self.last_down = None;
        let profile = self
            .app_class
            .as_deref()
            .filter(|a| self.base.profile_for(a).is_some());
        eprintln!("wacomd: profile {profile:?} (app {:?})", self.app_class);
        self.publish();
    }

    fn refresh_monitors(&mut self) {
        self.last_monitor_poll = Instant::now();
        let Some(monitors) = hyprland::monitors() else {
            return;
        };
        if monitors.is_empty() || monitors == self.monitors {
            return;
        }
        let layout = layout_of(&monitors);
        self.monitors = monitors;
        self.layout = layout;
        self.mapping = build_mapping(&self.effective, &self.monitors, self.layout);
        self.publish();
    }

    fn handle_report(&mut self, buf: &[u8]) {
        match parse_input(buf) {
            Ok(InputReport::Pen(p)) => self.on_pen(p),
            Ok(InputReport::Pad(p)) => self.on_pad(p.keys),
            Ok(InputReport::Battery(_)) | Ok(InputReport::Other { .. }) => {}
            Err(_) => {}
        }
    }

    fn on_pen(&mut self, p: PenReport) {
        self.pps_count += 1;
        self.telemetry.x = p.x;
        self.telemetry.y = p.y;
        self.telemetry.raw_pressure = p.pressure;
        self.telemetry.tilt_x = p.tilt_x;
        self.telemetry.tilt_y = p.tilt_y;
        self.telemetry.distance = p.distance;
        self.telemetry.in_range = p.in_range();
        self.telemetry.tip = p.tip();
        self.telemetry.barrel = p.barrel();
        self.telemetry.barrel2 = p.barrel2();
        self.telemetry.eraser = p.eraser();

        if self.scroll_mode {
            self.on_scroll(&p);
            return;
        }

        if !p.in_range() {
            self.leave_proximity();
            return;
        }

        let mut pos = self.mapping.map(f64::from(p.x), f64::from(p.y));
        if self.precision {
            if let Some(prev) = self.last_emitted {
                pos = apply_precision(prev, pos, PRECISION_FACTOR);
            }
        }
        if p.tip() && !self.pen.tip {
            let now = Instant::now();
            let pen_cfg = &self.effective.pen;
            if let Some((when, previous)) = self.last_down {
                let near = (previous.0 - pos.0).hypot(previous.1 - pos.1)
                    <= f64::from(pen_cfg.double_click_distance);
                let in_time =
                    now.duration_since(when).as_millis() as u64 <= pen_cfg.double_click_time_ms;
                if near && in_time {
                    pos = previous;
                }
            }
            self.last_down = Some((now, pos));
        }
        self.last_emitted = Some(pos);
        let (ux, uy) = to_uinput(pos, self.layout);
        let pressure = self.lut.eval(p.pressure);
        self.telemetry.pressure = pressure;

        let pointer_mode = self.effective.pen.pointer_mode;
        let mut events = Vec::with_capacity(12);
        if pointer_mode {
            self.pen.in_range = true;
            events.push(abs_event(AbsoluteAxisCode::ABS_X, i32::from(ux)));
            events.push(abs_event(AbsoluteAxisCode::ABS_Y, i32::from(uy)));
            if p.tip() != self.pen.tip {
                events.push(key_event(KeyCode::BTN_LEFT, p.tip()));
                self.pen.tip = p.tip();
            }
        } else {
            let eraser = p.eraser();
            if eraser != self.pen.tool_rubber {
                events.push(key_event(KeyCode::BTN_TOOL_RUBBER, eraser));
                self.pen.tool_rubber = eraser;
            }
            let pen_tool = !eraser;
            if pen_tool != self.pen.tool_pen {
                events.push(key_event(KeyCode::BTN_TOOL_PEN, pen_tool));
                self.pen.tool_pen = pen_tool;
            }
            self.pen.in_range = true;
            events.push(abs_event(AbsoluteAxisCode::ABS_X, i32::from(ux)));
            events.push(abs_event(AbsoluteAxisCode::ABS_Y, i32::from(uy)));
            events.push(abs_event(
                AbsoluteAxisCode::ABS_PRESSURE,
                i32::from(pressure.min(PRESSURE_MAX)),
            ));
            if p.tip() != self.pen.tip {
                events.push(key_event(KeyCode::BTN_TOUCH, p.tip()));
                self.pen.tip = p.tip();
            }
        }

        let mut actions = Vec::new();
        self.side_button(
            p.barrel(),
            self.effective.pen.button1.clone(),
            KeyCode::BTN_STYLUS,
            !pointer_mode,
            &mut events,
            &mut actions,
        );
        self.side_button(
            p.barrel2(),
            self.effective.pen.button2.clone(),
            KeyCode::BTN_STYLUS2,
            !pointer_mode,
            &mut events,
            &mut actions,
        );

        if !events.is_empty() {
            if pointer_mode {
                self.emit_pointer(&events);
            } else {
                self.emit_stylus(&events);
            }
        }
        for action in actions {
            self.run_action(action);
        }
    }

    /// `Disabled` passes the native stylus button through; any other action is
    /// executed on the press edge and the native button stays released.
    fn side_button(
        &mut self,
        pressed: bool,
        action: Action,
        code: KeyCode,
        emit_native: bool,
        events: &mut Vec<InputEvent>,
        actions: &mut Vec<Action>,
    ) {
        let previous = match code {
            KeyCode::BTN_STYLUS => self.pen.barrel,
            _ => self.pen.barrel2,
        };
        if pressed == previous {
            return;
        }
        match code {
            KeyCode::BTN_STYLUS => self.pen.barrel = pressed,
            _ => self.pen.barrel2 = pressed,
        }

        let native = matches!(action, Action::Disabled);
        let target = native && pressed;
        let emitted = match code {
            KeyCode::BTN_STYLUS => self.pen.native_stylus,
            _ => self.pen.native_stylus2,
        };
        if target != emitted {
            if emit_native {
                events.push(key_event(code, target));
            }
            match code {
                KeyCode::BTN_STYLUS => self.pen.native_stylus = target,
                _ => self.pen.native_stylus2 = target,
            }
        }
        if !native && pressed {
            actions.push(action);
        }
    }

    fn on_pad(&mut self, keys: u8) {
        let changed = keys ^ self.pad_keys;
        self.pad_keys = keys;
        self.telemetry.keys = keys;
        if changed == 0 {
            return;
        }
        for i in 0..4u8 {
            if changed & (1 << i) == 0 {
                continue;
            }
            let pressed = keys & (1 << i) != 0;
            self.shared.broadcast(&Event::Pad { index: i, pressed });
            if pressed {
                let action = self.effective.expresskeys[i as usize].clone();
                self.run_action(action);
            }
        }
    }

    fn on_scroll(&mut self, p: &PenReport) {
        if !p.in_range() {
            self.last_scroll = None;
            if self.scroll_click {
                self.runner.set_button(KeyCode::BTN_LEFT, false);
                self.scroll_click = false;
            }
            self.pen.tip = false;
            self.telemetry.tip = false;
            return;
        }
        let pos = self.mapping.map(f64::from(p.x), f64::from(p.y));
        if let Some(last) = self.last_scroll {
            let dx = pos.0 - last.0;
            let dy = pos.1 - last.1;
            self.wheel_accum.0 += dx;
            self.wheel_accum.1 -= dy;
        }
        self.last_scroll = Some(pos);

        let vertical = (self.wheel_accum.1 / SCROLL_STEP).trunc() as i32;
        let horizontal = (self.wheel_accum.0 / SCROLL_STEP).trunc() as i32;
        if vertical != 0 || horizontal != 0 {
            self.wheel_accum.0 -= f64::from(horizontal) * SCROLL_STEP;
            self.wheel_accum.1 -= f64::from(vertical) * SCROLL_STEP;
            self.runner.wheel(vertical, horizontal);
        }
        if p.tip() && !self.pen.tip {
            self.runner.set_button(KeyCode::BTN_LEFT, true);
            self.scroll_click = true;
        } else if !p.tip() && self.pen.tip && self.scroll_click {
            self.runner.set_button(KeyCode::BTN_LEFT, false);
            self.scroll_click = false;
        }
        self.pen.tip = p.tip();
        self.pen.in_range = true;
    }

    fn run_action(&mut self, action: Action) {
        if run_external(&mut self.runner, &action) {
            return;
        }
        match action {
            Action::Precision => {
                self.precision = !self.precision;
                self.last_emitted = None;
            }
            Action::ToggleMonitor => self.toggle_monitor(),
            Action::RadialMenu => self.shared.broadcast(&Event::Radial),
            Action::Mouse(MouseAction::PanScroll) => self.set_scroll_mode(!self.scroll_mode),
            Action::Disabled => {}
            _ => {}
        }
    }

    fn toggle_monitor(&mut self) {
        let names: Vec<String> = self.monitors.iter().map(|m| m.name.clone()).collect();
        if names.is_empty() {
            return;
        }
        let next = match &self.base.mapping.screen {
            ScreenSelection::All => ScreenSelection::Monitor(names[0].clone()),
            ScreenSelection::Monitor(current) => match names.iter().position(|n| n == current) {
                Some(i) if i + 1 < names.len() => ScreenSelection::Monitor(names[i + 1].clone()),
                _ => ScreenSelection::All,
            },
        };
        self.base.mapping.screen = next;
        if let Err(e) = config::save(&self.config_path, &self.base) {
            eprintln!("wacomd: cannot save config: {e}");
        }
        if let Ok(mut guard) = self.shared.config.lock() {
            *guard = self.base.clone();
        }
        self.shared.broadcast(&Event::Config(self.base.clone()));
        self.apply_effective();
    }

    fn set_scroll_mode(&mut self, on: bool) {
        if on == self.scroll_mode {
            return;
        }
        if on {
            self.leave_proximity();
            self.last_scroll = None;
            self.wheel_accum = (0.0, 0.0);
        } else {
            self.last_emitted = None;
            if self.scroll_click {
                self.runner.set_button(KeyCode::BTN_LEFT, false);
                self.scroll_click = false;
            }
        }
        self.scroll_mode = on;
    }

    fn leave_proximity(&mut self) {
        if self.effective.pen.pointer_mode {
            if !self.pen.in_range && !self.pen.tip {
                return;
            }
            if self.pen.tip {
                self.emit_pointer(&[key_event(KeyCode::BTN_LEFT, false)]);
            }
            self.pen = PenState::default();
            self.last_emitted = None;
            self.telemetry.tip = false;
            self.telemetry.barrel = false;
            self.telemetry.barrel2 = false;
            self.telemetry.in_range = false;
            self.telemetry.pressure = 0;
            return;
        }
        if !self.pen.in_range && !self.pen.tool_pen && !self.pen.tool_rubber {
            return;
        }
        let mut events = Vec::with_capacity(6);
        events.push(abs_event(AbsoluteAxisCode::ABS_PRESSURE, 0));
        if self.pen.tip {
            events.push(key_event(KeyCode::BTN_TOUCH, false));
        }
        if self.pen.native_stylus {
            events.push(key_event(KeyCode::BTN_STYLUS, false));
        }
        if self.pen.native_stylus2 {
            events.push(key_event(KeyCode::BTN_STYLUS2, false));
        }
        if self.pen.tool_pen {
            events.push(key_event(KeyCode::BTN_TOOL_PEN, false));
        }
        if self.pen.tool_rubber {
            events.push(key_event(KeyCode::BTN_TOOL_RUBBER, false));
        }
        self.emit_stylus(&events);
        self.pen = PenState::default();
        self.last_emitted = None;
        self.telemetry.tip = false;
        self.telemetry.barrel = false;
        self.telemetry.barrel2 = false;
        self.telemetry.in_range = false;
        self.telemetry.pressure = 0;
    }

    fn release_all(&mut self) {
        self.leave_proximity();
        if self.scroll_click {
            self.runner.set_button(KeyCode::BTN_LEFT, false);
            self.scroll_click = false;
        }
    }

    fn emit_stylus(&mut self, events: &[InputEvent]) {
        if let Err(e) = self.stylus.emit(events) {
            eprintln!("wacomd: stylus emit failed: {e}");
        }
    }

    fn emit_pointer(&mut self, events: &[InputEvent]) {
        if let Err(e) = self.pointer.emit(events) {
            eprintln!("wacomd: pointer emit failed: {e}");
        }
    }

    fn update_pps(&mut self) {
        let elapsed = self.pps_start.elapsed();
        if elapsed >= Duration::from_secs(1) {
            self.telemetry.pps = self.pps_count as f32 / elapsed.as_secs_f32();
            self.pps_count = 0;
            self.pps_start = Instant::now();
        }
    }

    fn publish(&mut self) {
        let status = Status {
            connected: self.device.is_some(),
            product: self.product.clone(),
            serial: self.serial.clone(),
            mode: self.mode,
            app_class: self.app_class.clone(),
            active_profile: self
                .app_class
                .as_deref()
                .filter(|a| self.base.profile_for(a).is_some())
                .map(str::to_string),
            pinned: self.pinned,
            monitors: self.monitors.clone(),
            layout: self.layout,
            telemetry: Some(self.telemetry.clone()),
            config_path: self.config_path.display().to_string(),
        };
        if let Ok(mut guard) = self.shared.status.lock() {
            *guard = status.clone();
        }
        if self.shared.has_subscribers() {
            self.shared
                .broadcast(&Event::Telemetry(self.telemetry.clone()));
            if self.last_status_broadcast.elapsed() >= Duration::from_secs(1) {
                self.shared.broadcast(&Event::Status(status));
                self.last_status_broadcast = Instant::now();
            }
        }
    }
}

fn build_mapping(effective: &EffectiveConfig, monitors: &[Monitor], layout: PixelRect) -> Mapping {
    let target = match &effective.mapping.screen {
        ScreenSelection::All => layout,
        ScreenSelection::Monitor(name) => monitors
            .iter()
            .find(|m| &m.name == name)
            .map(|m| PixelRect::new(m.x, m.y, m.width, m.height))
            .unwrap_or(layout),
    };
    Mapping {
        tablet_area: effective.mapping.tablet_area.sanitized(),
        target,
        layout,
        orientation: effective.mapping.orientation,
        force_proportions: effective.mapping.force_proportions,
    }
}

/// Absolute pointer used by profiles with `pointer_mode`: toolkits without
/// tablet support (winit/egui) only ever see pointer events.
fn build_pointer() -> io::Result<VirtualDevice> {
    let mut keys = AttributeSet::<KeyCode>::new();
    keys.insert(KeyCode::BTN_LEFT);
    let mut builder = VirtualDevice::builder()?
        .name("CTL-4100 Userspace Pointer")
        .input_id(InputId::new(BusType::BUS_VIRTUAL, 0x1209, 0x0003, 1))
        .with_keys(&keys)?;
    for code in [AbsoluteAxisCode::ABS_X, AbsoluteAxisCode::ABS_Y] {
        let resolution = if code == AbsoluteAxisCode::ABS_X {
            216
        } else {
            345
        };
        let info = AbsInfo::new(0, 0, 32_767, 0, 0, resolution);
        builder = builder.with_absolute_axis(&UinputAbsSetup::new(code, info))?;
    }
    builder.build()
}

fn build_stylus() -> io::Result<VirtualDevice> {
    let mut keys = AttributeSet::<KeyCode>::new();
    for key in [
        KeyCode::BTN_TOOL_PEN,
        KeyCode::BTN_TOOL_RUBBER,
        KeyCode::BTN_TOUCH,
        KeyCode::BTN_STYLUS,
        KeyCode::BTN_STYLUS2,
    ] {
        keys.insert(key);
    }
    let mut builder = VirtualDevice::builder()?
        .name("CTL-4100 Userspace Pen")
        .input_id(InputId::new(BusType::BUS_VIRTUAL, 0x1209, 0x0001, 1))
        .with_keys(&keys)?;
    // libinput rejects tablet devices whose X/Y axes have resolution 0.
    // 100 units/mm is the CTL-4100's physical resolution (2540 lpi); the
    // emitted values themselves are layout-normalized 0..=32767.
    for code in [
        AbsoluteAxisCode::ABS_X,
        AbsoluteAxisCode::ABS_Y,
        AbsoluteAxisCode::ABS_PRESSURE,
    ] {
        let pressure = code == AbsoluteAxisCode::ABS_PRESSURE;
        let max = if pressure {
            i32::from(PRESSURE_MAX)
        } else {
            32_767
        };
        // Match the real 152 x 95 mm active area over the 0..=32767 range.
        let resolution = match code {
            AbsoluteAxisCode::ABS_X => 216, // 32767 / 152 mm
            AbsoluteAxisCode::ABS_Y => 345, // 32767 / 95 mm
            _ => 0,
        };
        let info = AbsInfo::new(0, 0, max, 0, 0, resolution);
        builder = builder.with_absolute_axis(&UinputAbsSetup::new(code, info))?;
    }
    builder.build()
}

fn key_event(code: KeyCode, down: bool) -> InputEvent {
    InputEvent::new(EventType::KEY.0, code.0, i32::from(down))
}

fn abs_event(code: AbsoluteAxisCode, value: i32) -> InputEvent {
    InputEvent::new(EventType::ABSOLUTE.0, code.0, value)
}
