//! Wacom Center-style companion app for the Intuos S (CTL-4100).

mod client;
mod panels;

use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use eframe::egui;
use egui::{Color32, Key, RichText};
use wacom_proto::config::{Action, Config, Modifier, Profile};
use wacom_proto::ipc::{Event, Request, Status, Telemetry};
use wacom_proto::EffectiveConfig;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Pen,
    Keys,
    Mapping,
    Diagnostics,
}

impl Tab {
    pub fn label(self) -> &'static str {
        match self {
            Tab::Pen => "Pen",
            Tab::Keys => "ExpressKeys",
            Tab::Mapping => "Mapping",
            Tab::Diagnostics => "Diagnostics",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CaptureTarget {
    Pen1,
    Pen2,
    Key(usize),
}

pub struct App {
    rx: Receiver<client::UiEvent>,
    tx: Sender<Request>,
    pub connected: bool,
    pub error: Option<String>,
    pub status: Option<Status>,
    pub config: Option<Config>,
    pub tab: Tab,
    pub telemetry: Telemetry,
    pub history: VecDeque<u16>,
    pub capture: Option<CaptureTarget>,
    pub profile_choice: Option<String>,
    save_at: Option<Instant>,
    pub radial: bool,
    radial_focus: bool,
    pub curve_drag: Option<usize>,
    pub last_telemetry: Instant,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(18, 19, 24);
        visuals.window_fill = Color32::from_rgb(24, 26, 32);
        visuals.extreme_bg_color = Color32::from_rgb(12, 13, 17);
        visuals.selection.bg_fill = Color32::from_rgb(58, 108, 190);
        cc.egui_ctx.set_visuals(visuals);
        let mut style = (*cc.egui_ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        cc.egui_ctx.set_style(style);
        let (tx, rx) = client::spawn();
        Self {
            tx,
            rx,
            connected: false,
            error: None,
            status: None,
            config: None,
            tab: Tab::Pen,
            telemetry: Telemetry::default(),
            history: VecDeque::with_capacity(160),
            capture: None,
            profile_choice: None,
            save_at: None,
            radial: false,
            radial_focus: false,
            curve_drag: None,
            last_telemetry: Instant::now() - Duration::from_secs(1),
        }
    }

    fn poll(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                client::UiEvent::Event(Event::Status(status)) => {
                    self.connected = true;
                    self.profile_choice = if status.pinned {
                        status.app_class.clone()
                    } else {
                        None
                    };
                    self.status = Some(status);
                    self.error = None;
                }
                client::UiEvent::Event(Event::Config(cfg)) => {
                    self.config = Some(cfg);
                }
                client::UiEvent::Event(Event::Telemetry(t)) => {
                    self.history.push_back(t.pressure);
                    while self.history.len() > 160 {
                        self.history.pop_front();
                    }
                    self.telemetry = t;
                    self.last_telemetry = Instant::now();
                }
                client::UiEvent::Event(Event::Pad { index, pressed }) => {
                    let bit = 1u8 << index;
                    if pressed {
                        self.telemetry.keys |= bit;
                    } else {
                        self.telemetry.keys &= !bit;
                    }
                }
                client::UiEvent::Event(Event::Radial) => {
                    self.radial = true;
                    self.radial_focus = true;
                }
                client::UiEvent::Event(Event::Error { message }) => {
                    self.error = Some(message);
                }
                client::UiEvent::Event(_) => {}
                client::UiEvent::Error(message) => {
                    self.error = Some(message);
                }
                client::UiEvent::Disconnected(message) => {
                    self.connected = false;
                    self.error = Some(message);
                }
            }
        }
    }

    pub fn mark_dirty(&mut self) {
        self.save_at = Some(Instant::now() + Duration::from_millis(250));
    }

    fn maybe_save(&mut self) {
        let Some(at) = self.save_at else {
            return;
        };
        if Instant::now() < at {
            return;
        }
        self.save_at = None;
        let Some(cfg) = self.config.clone() else {
            return;
        };
        if self.tx.send(Request::SetConfig(cfg)).is_err() {
            self.error = Some("IPC worker stopped".into());
        }
    }

    fn handle_capture(&mut self, ctx: &egui::Context) {
        let Some(target) = self.capture else {
            return;
        };
        ctx.request_repaint();
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.capture = None;
            return;
        }
        let mut captured = None;
        ctx.input(|i| {
            for event in &i.events {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } = event
                {
                    if *key == Key::Escape {
                        continue;
                    }
                    if let Some(name) = egui_key_name(*key) {
                        let mut mods = Vec::new();
                        if modifiers.ctrl {
                            mods.push(Modifier::Ctrl);
                        }
                        if modifiers.shift {
                            mods.push(Modifier::Shift);
                        }
                        if modifiers.alt {
                            mods.push(Modifier::Alt);
                        }
                        if modifiers.mac_cmd {
                            mods.push(Modifier::Super);
                        }
                        captured = Some((mods, name));
                        break;
                    }
                }
            }
        });
        if let Some((mods, key)) = captured {
            if let Some(cfg) = self.config.as_mut() {
                set_action(cfg, target, Action::Key { mods, key });
                self.mark_dirty();
            }
            self.capture = None;
        }
    }

    fn create_profile(&mut self, app: String) {
        let Some(cfg) = self.config.as_mut() else {
            return;
        };
        cfg.profiles.insert(
            app.clone(),
            Profile {
                pen: Some(cfg.pen.clone()),
                expresskeys: Some(cfg.expresskeys.clone()),
                mapping: Some(cfg.mapping.clone()),
            },
        );
        self.mark_dirty();
        if self
            .tx
            .send(Request::PinProfile(Some(app.clone())))
            .is_err()
        {
            self.error = Some("IPC worker stopped".into());
        }
        self.profile_choice = Some(app);
    }

    fn header(&mut self, ctx: &egui::Context) {
        let mut create_profile: Option<String> = None;
        egui::TopBottomPanel::top("header")
            .frame(
                egui::Frame::default()
                    .fill(Color32::from_rgb(23, 25, 31))
                    .inner_margin(egui::Margin::symmetric(16, 12)),
            )
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("Wacom Intuos S")
                                .size(22.0)
                                .strong()
                                .color(Color32::from_rgb(235, 238, 245)),
                        );
                        ui.label(
                            RichText::new("CTL-4100 · userspace driver")
                                .small()
                                .color(muted()),
                        );
                    });
                    ui.add_space(8.0);
                    let (text, color) = if self.connected {
                        ("daemon online", Color32::from_rgb(90, 200, 130))
                    } else {
                        ("daemon offline", Color32::from_rgb(230, 120, 110))
                    };
                    badge(ui, text, color);
                    let (text, color) =
                        if self.status.as_ref().map(|s| s.connected).unwrap_or(false) {
                            ("tablet connected", Color32::from_rgb(90, 200, 130))
                        } else {
                            ("no tablet", Color32::from_rgb(220, 170, 90))
                        };
                    badge(ui, text, color);
                    if self.radial {
                        badge(ui, "radial menu", accent());
                    }
                });
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Profile").small().color(muted()));
                    let mut choice = self.profile_choice.clone();
                    let selected = choice.clone().unwrap_or_else(|| "Follow focus".to_string());
                    egui::ComboBox::from_id_salt("profile")
                        .selected_text(selected)
                        .width(160.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut choice, None, "Follow focus");
                            if let Some(cfg) = &self.config {
                                for key in cfg.profiles.keys() {
                                    ui.selectable_value(&mut choice, Some(key.clone()), key);
                                }
                            }
                        });
                    if choice != self.profile_choice {
                        if self.tx.send(Request::PinProfile(choice.clone())).is_err() {
                            self.error = Some("IPC worker stopped".into());
                        }
                        self.profile_choice = choice;
                    }
                    if let Some(app) = self
                        .status
                        .as_ref()
                        .and_then(|s| s.app_class.as_ref())
                        .filter(|app| {
                            self.config
                                .as_ref()
                                .map(|c| c.profile_for(app).is_none())
                                .unwrap_or(false)
                        })
                    {
                        if ui.small_button(format!("+ {app}")).clicked() {
                            create_profile = Some(app.clone());
                        }
                    }
                    if ui.button("Radial menu").clicked() {
                        self.radial = true;
                        self.radial_focus = true;
                    }
                    if let Some(status) = &self.status {
                        let app = status.app_class.as_deref().unwrap_or("—");
                        let profile = status.active_profile.as_deref().unwrap_or("global");
                        ui.label(
                            RichText::new(format!("{profile} · {app}"))
                                .small()
                                .color(muted()),
                        );
                    }
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    for tab in [Tab::Pen, Tab::Keys, Tab::Mapping, Tab::Diagnostics] {
                        let selected = self.tab == tab;
                        if ui
                            .add(
                                egui::Button::new(RichText::new(tab.label()).size(14.0))
                                    .fill(if selected {
                                        Color32::from_rgb(45, 74, 122)
                                    } else {
                                        Color32::TRANSPARENT
                                    })
                                    .corner_radius(egui::CornerRadius::same(7))
                                    .min_size(egui::vec2(96.0, 30.0)),
                            )
                            .clicked()
                        {
                            self.tab = tab;
                        }
                    }
                });
            });
        if let Some(app) = create_profile {
            self.create_profile(app);
        }
    }

    fn offline(&self, ui: &mut egui::Ui) {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("wacomd is not running")
                    .size(20.0)
                    .color(Color32::from_rgb(230, 120, 110)),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    self.error
                        .clone()
                        .unwrap_or_else(|| "Waiting for the daemon…".into()),
                )
                .color(muted()),
            );
            ui.add_space(14.0);
            if ui.button("Retry").clicked() {
                // The reader reconnects every 2 s; nothing else to do.
            }
            ui.add_space(18.0);
            if let Some(status) = &self.status {
                ui.label(
                    RichText::new(format!("config: {}", status.config_path))
                        .small()
                        .color(muted()),
                );
            }
            ui.label(
                RichText::new("Start with: systemctl --user start wacomd")
                    .small()
                    .color(muted()),
            );
        });
    }

    fn radial_overlay(&mut self, ctx: &egui::Context) {
        if !self.radial {
            return;
        }
        if self.radial_focus {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            self.radial_focus = false;
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.radial = false;
            return;
        }
        let screen = ctx.screen_rect();
        let mut chosen: Option<Action> = None;
        egui::Area::new(egui::Id::new("radial"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                ui.allocate_ui_with_layout(
                    screen.size(),
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        let (rect, _) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
                        ui.painter()
                            .rect_filled(rect, 0, Color32::from_black_alpha(170));
                        let center = rect.center();
                        ui.painter().circle_stroke(
                            center,
                            168.0,
                            egui::Stroke::new(1.0_f32, Color32::from_white_alpha(40)),
                        );
                        ui.painter().text(
                            center + egui::vec2(0.0, 168.0 + 24.0),
                            egui::Align2::CENTER_CENTER,
                            "click an action · Esc closes",
                            egui::FontId::proportional(12.0),
                            muted(),
                        );
                        let items: [(&str, Action); 6] = [
                            ("Undo", Action::key(&[Modifier::Ctrl], "z")),
                            ("Redo", Action::key(&[Modifier::Ctrl, Modifier::Shift], "z")),
                            ("Copy", Action::key(&[Modifier::Ctrl], "c")),
                            ("Paste", Action::key(&[Modifier::Ctrl], "v")),
                            (
                                "Zoom in",
                                Action::key(&[Modifier::Ctrl, Modifier::Shift], "equal"),
                            ),
                            ("Zoom out", Action::key(&[Modifier::Ctrl], "minus")),
                        ];
                        for (i, (label, action)) in items.into_iter().enumerate() {
                            let angle = -std::f32::consts::FRAC_PI_2
                                + i as f32 * std::f32::consts::TAU / 6.0;
                            let pos = center + egui::vec2(angle.cos(), angle.sin()) * 168.0;
                            let rect = egui::Rect::from_center_size(pos, egui::vec2(124.0, 44.0));
                            let button = egui::Button::new(RichText::new(label).size(14.0))
                                .corner_radius(egui::CornerRadius::same(22))
                                .fill(Color32::from_rgb(34, 42, 58));
                            if ui.put(rect, button).clicked() {
                                chosen = Some(action);
                            }
                        }
                        if ui
                            .put(
                                egui::Rect::from_center_size(center, egui::vec2(96.0, 36.0)),
                                egui::Button::new("Close")
                                    .corner_radius(egui::CornerRadius::same(18)),
                            )
                            .clicked()
                        {
                            self.radial = false;
                        }
                    },
                );
            });
        if let Some(action) = chosen {
            if self.tx.send(Request::RunAction(action)).is_err() {
                self.error = Some("IPC worker stopped".into());
            }
            self.radial = false;
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();
        self.handle_capture(ctx);
        self.maybe_save();
        self.header(ctx);

        egui::CentralPanel::default()
            .frame(
                egui::Frame::default()
                    .fill(Color32::from_rgb(18, 19, 24))
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(ctx, |ui| {
                if !self.connected || self.config.is_none() {
                    self.offline(ui);
                    return;
                }
                let mut cfg = self.config.take().expect("checked above");
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if let Some(app) = self.profile_choice.clone() {
                            let mut draft = draft_config(&cfg, &app);
                            draw_tabs(ui, self, &mut draft);
                            let after = EffectiveConfig {
                                pen: draft.pen,
                                expresskeys: draft.expresskeys,
                                mapping: draft.mapping,
                            };
                            if apply_draft(&mut cfg, &app, after) {
                                self.mark_dirty();
                            }
                        } else {
                            draw_tabs(ui, self, &mut cfg);
                        }
                    });
                self.config = Some(cfg);
            });

        self.radial_overlay(ctx);
        ctx.request_repaint_after(Duration::from_millis(50));
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 720.0])
            .with_min_inner_size([780.0, 560.0])
            .with_title("Wacom Intuos S (CTL-4100)")
            .with_app_id("wacom-gui"),
        ..Default::default()
    };
    eframe::run_native(
        "wacom-gui",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

fn draw_tabs(ui: &mut egui::Ui, app: &mut App, cfg: &mut Config) {
    match app.tab {
        Tab::Pen => panels::pen_tab(ui, app, cfg),
        Tab::Keys => panels::keys_tab(ui, app, cfg),
        Tab::Mapping => panels::mapping_tab(ui, app, cfg),
        Tab::Diagnostics => panels::diagnostics_tab(ui, app, cfg),
    }
}

fn apply_draft(cfg: &mut Config, app: &str, after: EffectiveConfig) -> bool {
    if after == cfg.effective(Some(app)) {
        return false;
    }
    let profile = cfg.profiles.entry(app.to_string()).or_default();
    profile.pen = Some(after.pen);
    profile.expresskeys = Some(after.expresskeys);
    profile.mapping = Some(after.mapping);
    true
}

fn draft_config(cfg: &Config, app: &str) -> Config {
    let effective = cfg.effective(Some(app));
    Config {
        version: cfg.version,
        pen: effective.pen,
        expresskeys: effective.expresskeys,
        mapping: effective.mapping,
        profiles: std::collections::BTreeMap::new(),
    }
}

fn set_action(cfg: &mut Config, target: CaptureTarget, action: Action) {
    match target {
        CaptureTarget::Pen1 => cfg.pen.button1 = action,
        CaptureTarget::Pen2 => cfg.pen.button2 = action,
        CaptureTarget::Key(i) => {
            if let Some(slot) = cfg.expresskeys.get_mut(i) {
                *slot = action;
            }
        }
    }
}

pub fn accent() -> Color32 {
    Color32::from_rgb(90, 160, 255)
}

pub fn muted() -> Color32 {
    Color32::from_rgb(140, 148, 165)
}

pub fn ok() -> Color32 {
    Color32::from_rgb(95, 200, 135)
}

fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::default()
        .fill(color.gamma_multiply(0.18))
        .stroke(egui::Stroke::new(1.0_f32, color.gamma_multiply(0.55)))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(text).small().color(color));
        });
}

/// Best-effort mapping from egui keys to the names understood by `wacomd`.
pub fn egui_key_name(key: Key) -> Option<String> {
    let name = match key {
        Key::ArrowDown => "down",
        Key::ArrowLeft => "left",
        Key::ArrowRight => "right",
        Key::ArrowUp => "up",
        Key::Tab => "tab",
        Key::Backspace => "backspace",
        Key::Enter => "enter",
        Key::Space => "space",
        Key::Insert => "insert",
        Key::Delete => "delete",
        Key::Home => "home",
        Key::End => "end",
        Key::PageUp => "pageup",
        Key::PageDown => "pagedown",
        Key::Colon => "semicolon",
        Key::Comma => "comma",
        Key::Backslash => "backslash",
        Key::Slash => "slash",
        Key::OpenBracket => "leftbrace",
        Key::CloseBracket => "rightbrace",
        Key::Backtick => "grave",
        Key::Minus => "minus",
        Key::Period => "period",
        Key::Plus => "plus",
        Key::Equals => "equal",
        Key::Semicolon => "semicolon",
        Key::Quote => "apostrophe",
        Key::Num0 => "0",
        Key::Num1 => "1",
        Key::Num2 => "2",
        Key::Num3 => "3",
        Key::Num4 => "4",
        Key::Num5 => "5",
        Key::Num6 => "6",
        Key::Num7 => "7",
        Key::Num8 => "8",
        Key::Num9 => "9",
        _ => {
            let debug = format!("{key:?}");
            if debug.starts_with('F') && debug[1..].chars().all(|c| c.is_ascii_digit()) {
                return Some(debug.to_lowercase());
            }
            if debug.len() == 1 && debug.chars().all(|c| c.is_ascii_uppercase()) {
                return Some(debug.to_lowercase());
            }
            return None;
        }
    };
    Some(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wacom_proto::config::PenConfig;

    #[test]
    fn draft_uses_profile_overrides() {
        let profile = Profile {
            pen: Some(PenConfig {
                tip_feel: 0.1,
                ..PenConfig::default()
            }),
            ..Profile::default()
        };
        let mut cfg = Config {
            pen: PenConfig {
                tip_feel: 0.9,
                ..PenConfig::default()
            },
            ..Config::default()
        };
        cfg.profiles.insert("krita".into(), profile);
        let draft = draft_config(&cfg, "krita");
        assert_eq!(draft.pen.tip_feel, 0.1);
        assert_eq!(draft.mapping, cfg.mapping);
    }

    #[test]
    fn apply_draft_writes_only_the_profile() {
        let mut cfg = Config::default();
        let mut draft = draft_config(&cfg, "krita");
        draft.pen.tip_feel = 0.2;
        let after = EffectiveConfig {
            pen: draft.pen,
            expresskeys: draft.expresskeys,
            mapping: draft.mapping,
        };
        assert!(apply_draft(&mut cfg, "krita", after));
        assert_eq!(cfg.pen.tip_feel, 0.5);
        assert_eq!(cfg.profiles["krita"].pen.as_ref().unwrap().tip_feel, 0.2);
        assert!(cfg.profiles["krita"].mapping.is_some());
    }

    #[test]
    fn apply_draft_is_a_noop_without_changes() {
        let mut cfg = Config::default();
        let after = cfg.effective(Some("krita"));
        assert!(!apply_draft(&mut cfg, "krita", after));
        assert!(cfg.profiles.is_empty());
    }

    #[test]
    fn maps_common_keys_from_egui() {
        assert_eq!(egui_key_name(Key::A).as_deref(), Some("a"));
        assert_eq!(egui_key_name(Key::F11).as_deref(), Some("f11"));
        assert_eq!(egui_key_name(Key::ArrowLeft).as_deref(), Some("left"));
        assert_eq!(egui_key_name(Key::Num3).as_deref(), Some("3"));
    }
}
