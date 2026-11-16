//! Tab contents and custom widgets.

use eframe::egui;
use egui::{Color32, CornerRadius, FontId, RichText, Sense, Stroke, StrokeKind, Ui, Vec2};

use wacom_proto::config::{Action, Config, Modifier, MouseAction, ScreenSelection};
use wacom_proto::transform::{Mapping, Orientation, PixelRect};
use wacom_proto::{CurvePoints, PRESSURE_MAX, TABLET_HEIGHT, TABLET_WIDTH};

use crate::{accent, muted, ok, CaptureTarget};

const CARD: Color32 = Color32::from_rgb(24, 26, 32);
const CARD_INNER: Color32 = Color32::from_rgb(30, 33, 41);

pub fn pen_tab(ui: &mut Ui, app: &mut crate::App, cfg: &mut Config) {
    ui.columns(2, |cols| {
        let left = &mut cols[0];
        section(left, "LP-1100K stylus", |ui| {
            draw_pen(ui, &app.telemetry);
        });
        section(left, "Live pressure", |ui| {
            live_pressure(ui, app);
        });

        let right = &mut cols[1];
        section(right, "Side buttons", |ui| {
            ui.label(RichText::new("Lower button").small().color(muted()));
            action_editor(ui, "pen1", &mut cfg.pen.button1, CaptureTarget::Pen1, app);
            ui.add_space(8.0);
            ui.label(RichText::new("Upper button").small().color(muted()));
            action_editor(ui, "pen2", &mut cfg.pen.button2, CaptureTarget::Pen2, app);
            ui.add_space(4.0);
            ui.label(
                RichText::new("Disabled passes BTN_STYLUS / BTN_STYLUS2 to apps.")
                    .small()
                    .color(muted()),
            );
        });
        section(right, "Tip feel", |ui| {
            let mut feel = cfg.pen.tip_feel * 100.0;
            if ui
                .add(
                    egui::Slider::new(&mut feel, 0.0..=100.0)
                        .suffix("%")
                        .text("Soft ↔ Firm"),
                )
                .changed()
            {
                cfg.pen.tip_feel = feel / 100.0;
                cfg.pen.custom_curve = None;
                app.mark_dirty();
            }
            curve_editor(ui, cfg, app);
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let mut distance = cfg.pen.double_click_distance;
                if ui
                    .add(
                        egui::DragValue::new(&mut distance)
                            .speed(0.5)
                            .range(0.0..=200.0)
                            .suffix(" px"),
                    )
                    .changed()
                {
                    cfg.pen.double_click_distance = distance;
                    app.mark_dirty();
                }
                ui.label(RichText::new("double-click snap").small().color(muted()));
                let mut time = cfg.pen.double_click_time_ms;
                if ui
                    .add(
                        egui::DragValue::new(&mut time)
                            .speed(10.0)
                            .range(100..=1500)
                            .suffix(" ms"),
                    )
                    .changed()
                {
                    cfg.pen.double_click_time_ms = time;
                    app.mark_dirty();
                }
            });
        });
    });
}

pub fn keys_tab(ui: &mut Ui, app: &mut crate::App, cfg: &mut Config) {
    section(ui, "CTL-4100 ExpressKeys", |ui| {
        draw_tablet(ui, &app.telemetry);
    });
    section(ui, "Actions", |ui| {
        for i in 0..4 {
            ui.horizontal(|ui| {
                let pressed = app.telemetry.keys & (1 << i) != 0;
                let color = if pressed { accent() } else { muted() };
                ui.label(
                    RichText::new(format!("{}", (b'A' + i as u8) as char))
                        .strong()
                        .size(18.0)
                        .color(color),
                );
                ui.add_space(6.0);
                action_editor(
                    ui,
                    &format!("key{i}"),
                    &mut cfg.expresskeys[i],
                    CaptureTarget::Key(i),
                    app,
                );
            });
            if i != 3 {
                ui.separator();
            }
        }
    });
    section(ui, "Radial menu", |ui| {
        ui.label(
            RichText::new("Assign “Radial menu” to any key, then pick an action from the overlay.")
                .small()
                .color(muted()),
        );
        if ui.button("Preview radial menu").clicked() {
            app.radial = true;
        }
    });
}

pub fn mapping_tab(ui: &mut Ui, app: &mut crate::App, cfg: &mut Config) {
    let monitors = app
        .status
        .as_ref()
        .map(|s| s.monitors.clone())
        .unwrap_or_default();
    let layout = app
        .status
        .as_ref()
        .map(|s| s.layout)
        .unwrap_or_else(|| PixelRect::new(0.0, 0.0, 1920.0, 1080.0));

    ui.columns(2, |cols| {
        let left = &mut cols[0];
        section(left, "Screen", |ui| {
            let mut screen = cfg.mapping.screen.clone();
            let selected = match &screen {
                ScreenSelection::All => "All monitors".to_string(),
                ScreenSelection::Monitor(name) => name.clone(),
            };
            egui::ComboBox::from_id_salt("screen")
                .selected_text(selected)
                .width(220.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut screen, ScreenSelection::All, "All monitors");
                    for monitor in &monitors {
                        ui.selectable_value(
                            &mut screen,
                            ScreenSelection::Monitor(monitor.name.clone()),
                            &monitor.name,
                        );
                    }
                });
            if screen != cfg.mapping.screen {
                cfg.mapping.screen = screen;
                app.mark_dirty();
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                for (value, label) in [
                    (Orientation::Deg0, "0°"),
                    (Orientation::Deg90, "90°"),
                    (Orientation::Deg180, "180°"),
                    (Orientation::Deg270, "270°"),
                ] {
                    ui.radio_value(&mut cfg.mapping.orientation, value, label);
                }
            });
            if ui
                .checkbox(&mut cfg.mapping.force_proportions, "Force proportions")
                .changed()
            {
                app.mark_dirty();
            }
        });

        let right = &mut cols[1];
        section(right, "Tablet area", |ui| {
            let mut area = cfg.mapping.tablet_area;
            let mut changed = false;
            egui::Grid::new("area-grid")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    for (label, value) in [
                        ("Left", &mut area.x),
                        ("Top", &mut area.y),
                        ("Width", &mut area.w),
                        ("Height", &mut area.h),
                    ] {
                        ui.label(label);
                        changed |= ui
                            .add(
                                egui::DragValue::new(value)
                                    .speed(0.005)
                                    .range(0.0..=1.0)
                                    .fixed_decimals(3),
                            )
                            .changed();
                        ui.end_row();
                    }
                });
            if changed {
                cfg.mapping.tablet_area = area.sanitized();
                app.mark_dirty();
            }
            let area = cfg.mapping.tablet_area;
            ui.label(
                RichText::new(format!(
                    "{:.0} × {:.0} mm active",
                    area.w * f64::from(TABLET_WIDTH) / 100.0,
                    area.h * f64::from(TABLET_HEIGHT) / 100.0
                ))
                .small()
                .color(muted()),
            );
            if ui.button("Reset to full tablet").clicked() {
                cfg.mapping.tablet_area = wacom_proto::NormRect::FULL;
                app.mark_dirty();
            }
        });
    });

    section(ui, "Preview", |ui| {
        draw_mapping(ui, cfg, &monitors, layout);
    });
}

pub fn diagnostics_tab(ui: &mut Ui, app: &mut crate::App, cfg: &Config) {
    let t = &app.telemetry;
    let status = app.status.as_ref();
    section(ui, "Device", |ui| {
        egui::Grid::new("diag-device")
            .num_columns(2)
            .spacing([20.0, 6.0])
            .show(ui, |ui| {
                ui.label("connection");
                ui.label(if status.map(|s| s.connected).unwrap_or(false) {
                    RichText::new("connected").color(ok())
                } else {
                    RichText::new("disconnected").color(Color32::from_rgb(230, 120, 110))
                });
                ui.end_row();
                ui.label("product");
                ui.label(status.map(|s| s.product.as_str()).unwrap_or("—"));
                ui.end_row();
                ui.label("serial");
                ui.label(status.and_then(|s| s.serial.as_deref()).unwrap_or("—"));
                ui.end_row();
                ui.label("mode");
                ui.label(format!("{:?}", status.map(|s| s.mode).unwrap_or_default()));
                ui.end_row();
                ui.label("profile");
                ui.label(
                    status
                        .and_then(|s| s.active_profile.as_deref())
                        .unwrap_or("global"),
                );
                ui.end_row();
                ui.label("app class");
                ui.label(status.and_then(|s| s.app_class.as_deref()).unwrap_or("—"));
                ui.end_row();
                ui.label("packet rate");
                ui.label(format!("{:.0} pps", t.pps));
                ui.end_row();
                ui.label("config");
                ui.label(
                    RichText::new(status.map(|s| s.config_path.as_str()).unwrap_or("—"))
                        .small()
                        .color(muted()),
                );
                ui.end_row();
            });
    });

    ui.columns(2, |cols| {
        let left = &mut cols[0];
        section(left, "Stylus", |ui| {
            live_pressure(ui, app);
            egui::Grid::new("diag-pen")
                .num_columns(2)
                .spacing([20.0, 6.0])
                .show(ui, |ui| {
                    ui.label("X");
                    ui.label(RichText::new(format!("{} / {}", t.x, TABLET_WIDTH)).monospace());
                    ui.end_row();
                    ui.label("Y");
                    ui.label(RichText::new(format!("{} / {}", t.y, TABLET_HEIGHT)).monospace());
                    ui.end_row();
                    ui.label("pressure");
                    ui.label(RichText::new(format!("{} / {PRESSURE_MAX}", t.pressure)).monospace());
                    ui.end_row();
                    ui.label("raw pressure");
                    ui.label(RichText::new(format!("{}", t.raw_pressure)).monospace());
                    ui.end_row();
                    ui.label("distance");
                    ui.label(RichText::new(format!("{}", t.distance)).monospace());
                    ui.end_row();
                    ui.label("tilt");
                    ui.label(RichText::new(format!("{}, {}", t.tilt_x, t.tilt_y)).monospace());
                    ui.end_row();
                });
        });
        let right = &mut cols[1];
        section(right, "Buttons", |ui| {
            egui::Grid::new("diag-buttons")
                .num_columns(2)
                .spacing([20.0, 6.0])
                .show(ui, |ui| {
                    for (label, value) in [
                        ("in range", t.in_range),
                        ("tip", t.tip),
                        ("barrel 1", t.barrel),
                        ("barrel 2", t.barrel2),
                        ("eraser", t.eraser),
                    ] {
                        ui.label(label);
                        ui.label(if value {
                            RichText::new("yes").color(ok())
                        } else {
                            RichText::new("no").color(muted())
                        });
                        ui.end_row();
                    }
                    ui.label("ExpressKeys");
                    ui.label(
                        RichText::new(format!("{:04b}", t.keys))
                            .monospace()
                            .color(if t.keys != 0 { accent() } else { muted() }),
                    );
                    ui.end_row();
                });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for i in 0..4 {
                    let on = t.keys & (1 << i) != 0;
                    ui.label(
                        RichText::new(format!("{}", (b'A' + i as u8) as char))
                            .strong()
                            .color(if on { accent() } else { muted() }),
                    );
                }
            });
        });
    });

    section(ui, "Mapping", |ui| {
        ui.label(
            RichText::new(format!(
                "{} · {}° · proportions {}",
                match &cfg.mapping.screen {
                    ScreenSelection::All => "all monitors".to_string(),
                    ScreenSelection::Monitor(name) => name.clone(),
                },
                cfg.mapping.orientation.degrees(),
                if cfg.mapping.force_proportions {
                    "on"
                } else {
                    "off"
                }
            ))
            .color(muted()),
        );
        if let Some(status) = status {
            for m in &status.monitors {
                ui.label(
                    RichText::new(format!(
                        "{}  {:.0}×{:.0} @ {:.0},{:.0}  scale {:.2}{}",
                        m.name,
                        m.width,
                        m.height,
                        m.x,
                        m.y,
                        m.scale,
                        if m.focused { "  (focused)" } else { "" }
                    ))
                    .monospace()
                    .small(),
                );
            }
        }
    });
}

fn section(ui: &mut Ui, title: &str, body: impl FnOnce(&mut Ui)) {
    ui.add_space(4.0);
    ui.label(RichText::new(title).size(15.0).strong());
    ui.add_space(2.0);
    egui::Frame::default()
        .fill(CARD)
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::same(12))
        .show(ui, body);
    ui.add_space(6.0);
}

fn live_pressure(ui: &mut Ui, app: &crate::App) {
    let t = &app.telemetry;
    ui.add(
        egui::ProgressBar::new(f32::from(t.pressure) / f32::from(PRESSURE_MAX))
            .text(format!("{} / {PRESSURE_MAX}", t.pressure))
            .fill(accent()),
    );
    ui.add_space(4.0);
    sparkline(ui, &app.history);
}

fn sparkline(ui: &mut Ui, history: &std::collections::VecDeque<u16>) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 56.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(6), CARD_INNER);
    if history.len() < 2 {
        return;
    }
    let step = rect.width() / (history.len().max(2) - 1) as f32;
    let points: Vec<egui::Pos2> = history
        .iter()
        .enumerate()
        .map(|(i, value)| {
            egui::pos2(
                rect.left() + i as f32 * step,
                rect.bottom()
                    - 4.0
                    - f32::from(*value) / f32::from(PRESSURE_MAX) * (rect.height() - 8.0),
            )
        })
        .collect();
    painter.add(egui::Shape::line(points, Stroke::new(1.5_f32, accent())));
}

fn draw_pen(ui: &mut Ui, t: &wacom_proto::ipc::Telemetry) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 280.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(8), CARD_INNER);
    let cx = rect.center().x;
    let top = rect.top() + 18.0;
    let bottom = rect.bottom() - 16.0;
    let body = egui::Rect::from_min_max(
        egui::pos2(cx - 24.0, top),
        egui::pos2(cx + 24.0, bottom - 54.0),
    );
    painter.rect_filled(body, CornerRadius::same(22), Color32::from_rgb(52, 56, 66));
    painter.rect_stroke(
        body,
        CornerRadius::same(22),
        Stroke::new(1.0_f32, Color32::from_rgb(90, 96, 110)),
        StrokeKind::Inside,
    );
    // Nib
    let nib = vec![
        egui::pos2(cx - 11.0, body.bottom()),
        egui::pos2(cx + 11.0, body.bottom()),
        egui::pos2(cx + 3.0, bottom),
        egui::pos2(cx - 3.0, bottom),
    ];
    painter.add(egui::Shape::convex_polygon(
        nib,
        if t.tip {
            accent()
        } else {
            Color32::from_rgb(180, 185, 195)
        },
        Stroke::NONE,
    ));
    // Side buttons (lower = barrel1, upper = barrel2)
    for (i, pressed) in [(0usize, t.barrel), (1, t.barrel2)] {
        let y = body.top() + 76.0 + i as f32 * 54.0;
        let button =
            egui::Rect::from_min_size(egui::pos2(cx - 24.0 - 13.0, y), Vec2::new(13.0, 38.0));
        painter.rect_filled(
            button,
            CornerRadius::same(4),
            if pressed {
                accent()
            } else {
                Color32::from_rgb(78, 84, 96)
            },
        );
    }
    // Eraser cap
    painter.rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(cx - 24.0, top),
            egui::pos2(cx + 24.0, top + 20.0),
        ),
        CornerRadius::same(8),
        Color32::from_rgb(96, 102, 116),
    );
    painter.text(
        egui::pos2(cx, top - 8.0),
        egui::Align2::CENTER_BOTTOM,
        "LP-1100K",
        FontId::proportional(12.0),
        muted(),
    );
    painter.text(
        egui::pos2(cx + 58.0, body.top() + 86.0),
        egui::Align2::LEFT_CENTER,
        if t.barrel { "lower ●" } else { "lower" },
        FontId::proportional(11.0),
        if t.barrel { accent() } else { muted() },
    );
    painter.text(
        egui::pos2(cx + 58.0, body.top() + 140.0),
        egui::Align2::LEFT_CENTER,
        if t.barrel2 { "upper ●" } else { "upper" },
        FontId::proportional(11.0),
        if t.barrel2 { accent() } else { muted() },
    );
    painter.text(
        egui::pos2(cx, bottom + 6.0),
        egui::Align2::CENTER_TOP,
        if t.tip { "tip pressed" } else { "tip" },
        FontId::proportional(11.0),
        if t.tip { accent() } else { muted() },
    );
}

fn draw_tablet(ui: &mut Ui, t: &wacom_proto::ipc::Telemetry) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 250.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(10), CARD_INNER);
    let pad = rect.shrink2(Vec2::new(40.0, 24.0));
    painter.rect_filled(pad, CornerRadius::same(10), Color32::from_rgb(38, 41, 50));
    painter.rect_stroke(
        pad,
        CornerRadius::same(10),
        Stroke::new(1.0_f32, Color32::from_rgb(80, 86, 100)),
        StrokeKind::Inside,
    );
    let area = egui::Rect::from_min_max(
        pad.min + Vec2::new(84.0, 46.0),
        pad.max - Vec2::new(26.0, 22.0),
    );
    painter.rect_stroke(
        area,
        CornerRadius::same(4),
        Stroke::new(1.0_f32, Color32::from_white_alpha(20)),
        StrokeKind::Inside,
    );
    painter.text(
        area.center(),
        egui::Align2::CENTER_CENTER,
        "152 × 95 mm",
        FontId::proportional(12.0),
        Color32::from_white_alpha(45),
    );
    for i in 0..4 {
        let on = t.keys & (1 << i) != 0;
        let key = egui::Rect::from_min_size(
            egui::pos2(pad.left() + 18.0 + i as f32 * 44.0, pad.top() + 12.0),
            Vec2::splat(32.0),
        );
        painter.rect_filled(
            key,
            CornerRadius::same(7),
            if on {
                accent()
            } else {
                Color32::from_rgb(56, 60, 72)
            },
        );
        painter.text(
            key.center(),
            egui::Align2::CENTER_CENTER,
            format!("{}", (b'A' + i as u8) as char),
            FontId::proportional(13.0),
            if on { Color32::BLACK } else { muted() },
        );
    }
    // Stylus position preview
    if t.in_range {
        let px = pad.left() + 84.0 + t.x as f32 / TABLET_WIDTH as f32 * area.width();
        let py = pad.top() + 46.0 + t.y as f32 / TABLET_HEIGHT as f32 * area.height();
        painter.circle_filled(
            egui::pos2(px, py),
            4.0 + 8.0 * f32::from(t.pressure) / f32::from(PRESSURE_MAX),
            Color32::from_rgba_unmultiplied(90, 160, 255, 200),
        );
    }
}

fn draw_mapping(
    ui: &mut Ui,
    cfg: &Config,
    monitors: &[wacom_proto::ipc::Monitor],
    layout: PixelRect,
) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 250.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(10), CARD_INNER);
    let inner = rect.shrink2(Vec2::new(24.0, 20.0));
    let scale = (inner.width() as f64 / layout.w)
        .min(inner.height() as f64 / layout.h)
        .max(0.0001);
    let origin = egui::pos2(
        inner.center().x - (layout.w * scale) as f32 / 2.0,
        inner.center().y - (layout.h * scale) as f32 / 2.0,
    );
    let to_ui = |x: f64, y: f64| {
        egui::pos2(
            origin.x + ((x - layout.x) * scale) as f32,
            origin.y + ((y - layout.y) * scale) as f32,
        )
    };
    let full = egui::Rect::from_min_max(
        to_ui(layout.x, layout.y),
        to_ui(layout.x + layout.w, layout.y + layout.h),
    );
    painter.rect_stroke(
        full,
        CornerRadius::same(4),
        Stroke::new(1.0_f32, Color32::from_white_alpha(35)),
        StrokeKind::Inside,
    );
    for (i, m) in monitors.iter().enumerate() {
        let r = egui::Rect::from_min_max(to_ui(m.x, m.y), to_ui(m.x + m.width, m.y + m.height));
        let fill = if i % 2 == 0 {
            Color32::from_rgb(34, 38, 48)
        } else {
            Color32::from_rgb(40, 45, 56)
        };
        painter.rect_filled(r.shrink(2.0), CornerRadius::same(4), fill);
        painter.text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            &m.name,
            FontId::proportional(12.0),
            muted(),
        );
    }

    let target = match &cfg.mapping.screen {
        ScreenSelection::All => layout,
        ScreenSelection::Monitor(name) => monitors
            .iter()
            .find(|m| &m.name == name)
            .map(|m| PixelRect::new(m.x, m.y, m.width, m.height))
            .unwrap_or(layout),
    };
    painter.rect_stroke(
        egui::Rect::from_min_max(
            to_ui(target.x, target.y),
            to_ui(target.x + target.w, target.y + target.h),
        ),
        CornerRadius::same(3),
        Stroke::new(2.0_f32, accent().gamma_multiply(0.8)),
        StrokeKind::Inside,
    );

    let mapping = Mapping {
        tablet_area: cfg.mapping.tablet_area,
        target,
        layout,
        orientation: cfg.mapping.orientation,
        force_proportions: cfg.mapping.force_proportions,
    };
    let corners: Vec<egui::Pos2> = mapping
        .corners()
        .iter()
        .map(|(x, y)| to_ui(*x, *y))
        .collect();
    painter.add(egui::Shape::convex_polygon(
        corners,
        Color32::from_rgba_unmultiplied(90, 160, 255, 40),
        Stroke::new(1.5_f32, accent()),
    ));
    painter.text(
        rect.left_top() + Vec2::new(14.0, 12.0),
        egui::Align2::LEFT_TOP,
        format!(
            "{}° · proportions {}",
            cfg.mapping.orientation.degrees(),
            if cfg.mapping.force_proportions {
                "on"
            } else {
                "off"
            }
        ),
        FontId::proportional(11.0),
        muted(),
    );
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Disabled,
    Key,
    Mouse,
    Precision,
    ToggleMonitor,
    Radial,
    Shell,
}

impl Kind {
    const ALL: [Kind; 7] = [
        Kind::Disabled,
        Kind::Key,
        Kind::Mouse,
        Kind::Precision,
        Kind::ToggleMonitor,
        Kind::Radial,
        Kind::Shell,
    ];

    fn of(action: &Action) -> Self {
        match action {
            Action::Disabled => Kind::Disabled,
            Action::Key { .. } => Kind::Key,
            Action::Mouse(_) => Kind::Mouse,
            Action::Precision => Kind::Precision,
            Action::ToggleMonitor => Kind::ToggleMonitor,
            Action::RadialMenu => Kind::Radial,
            Action::Shell { .. } => Kind::Shell,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Kind::Disabled => "Disabled",
            Kind::Key => "Keystroke",
            Kind::Mouse => "Mouse",
            Kind::Precision => "Precision mode",
            Kind::ToggleMonitor => "Toggle monitor",
            Kind::Radial => "Radial menu",
            Kind::Shell => "Shell command",
        }
    }

    fn default_action(self) -> Action {
        match self {
            Kind::Disabled => Action::Disabled,
            Kind::Key => Action::key(&[Modifier::Ctrl], "z"),
            Kind::Mouse => Action::Mouse(MouseAction::Left),
            Kind::Precision => Action::Precision,
            Kind::ToggleMonitor => Action::ToggleMonitor,
            Kind::Radial => Action::RadialMenu,
            Kind::Shell => Action::Shell {
                command: "notify-send wacom".into(),
            },
        }
    }
}

fn action_editor(
    ui: &mut Ui,
    id: &str,
    action: &mut Action,
    target: CaptureTarget,
    app: &mut crate::App,
) {
    let current = Kind::of(action);
    let mut kind = current;
    egui::ComboBox::from_id_salt(id)
        .width(150.0)
        .selected_text(current.label())
        .show_ui(ui, |ui| {
            for k in Kind::ALL {
                ui.selectable_value(&mut kind, k, k.label());
            }
        });
    if kind != current {
        *action = kind.default_action();
        app.mark_dirty();
    }
    match action {
        Action::Key { mods, key } => {
            let capturing = app.capture == Some(target);
            let text = if capturing {
                "Press keys…"
            } else {
                "Capture"
            };
            if ui.button(text).clicked() {
                app.capture = Some(target);
            }
            ui.label(RichText::new(key.as_str()).monospace());
            for modifier in [
                Modifier::Ctrl,
                Modifier::Shift,
                Modifier::Alt,
                Modifier::Super,
            ] {
                let mut on = mods.contains(&modifier);
                if ui.checkbox(&mut on, format!("{modifier:?}")).changed() {
                    if on {
                        mods.push(modifier);
                    } else {
                        mods.retain(|m| *m != modifier);
                    }
                    app.mark_dirty();
                }
            }
        }
        Action::Mouse(mouse) => {
            let mut selected = *mouse;
            egui::ComboBox::from_id_salt(format!("{id}-mouse"))
                .width(150.0)
                .selected_text(format!("{mouse:?}"))
                .show_ui(ui, |ui| {
                    for (value, label) in [
                        (MouseAction::Left, "Left click"),
                        (MouseAction::Right, "Right click"),
                        (MouseAction::Middle, "Middle click"),
                        (MouseAction::DoubleClick, "Double click"),
                        (MouseAction::ScrollUp, "Scroll up"),
                        (MouseAction::ScrollDown, "Scroll down"),
                        (MouseAction::PanScroll, "Pan / scroll mode"),
                    ] {
                        ui.selectable_value(&mut selected, value, label);
                    }
                });
            if selected != *mouse {
                *mouse = selected;
                app.mark_dirty();
            }
        }
        Action::Shell { command } => {
            if ui.text_edit_singleline(command).changed() {
                app.mark_dirty();
            }
        }
        _ => {}
    }
}

fn curve_editor(ui: &mut Ui, cfg: &mut Config, app: &mut crate::App) {
    let curve = cfg.pen.curve();
    let mut points = curve.points();
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width().min(420.0), 220.0),
        Sense::click_and_drag(),
    );
    let plot = rect.shrink2(Vec2::new(18.0, 14.0));
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(8), CARD_INNER);
    for i in 0..=4 {
        let f = i as f32 / 4.0;
        let x = plot.left() + f * plot.width();
        let y = plot.bottom() - f * plot.height();
        painter.line_segment(
            [egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())],
            Stroke::new(1.0_f32, Color32::from_white_alpha(10)),
        );
        painter.line_segment(
            [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
            Stroke::new(1.0_f32, Color32::from_white_alpha(10)),
        );
    }
    let to_screen = |p: [f32; 2]| {
        egui::pos2(
            plot.left() + p[0] * plot.width(),
            plot.bottom() - p[1] * plot.height(),
        )
    };
    let samples: Vec<egui::Pos2> = (0..=96)
        .map(|i| {
            let x = i as f32 / 96.0;
            to_screen([x, curve.eval(x)])
        })
        .collect();
    painter.add(egui::Shape::line(samples, Stroke::new(2.0_f32, accent())));

    let custom = cfg.pen.custom_curve.is_some();
    let handles = [points.p1, points.p2];
    for (i, handle) in handles.iter().enumerate() {
        let pos = to_screen(*handle);
        let active = app.curve_drag == Some(i);
        painter.circle_filled(
            pos,
            if active { 7.0 } else { 5.0 },
            if custom {
                accent()
            } else {
                Color32::from_white_alpha(60)
            },
        );
        painter.text(
            pos + Vec2::new(10.0, -10.0),
            egui::Align2::LEFT_BOTTOM,
            format!("P{}", i + 1),
            FontId::proportional(10.0),
            muted(),
        );
    }

    if response.drag_started() {
        if let Some(pos) = response.interact_pointer_pos() {
            let d1 = pos.distance(to_screen(points.p1));
            let d2 = pos.distance(to_screen(points.p2));
            app.curve_drag = if d1.min(d2) < 18.0 {
                Some(if d1 <= d2 { 0 } else { 1 })
            } else {
                None
            };
        }
    }
    if response.dragged() {
        if let (Some(index), Some(pos)) = (app.curve_drag, response.interact_pointer_pos()) {
            let nx = ((pos.x - plot.left()) / plot.width()).clamp(0.0, 1.0);
            let ny = (1.0 - (pos.y - plot.top()) / plot.height()).clamp(0.0, 1.0);
            let mut arr = [points.p1, points.p2];
            arr[index] = [nx, ny];
            cfg.pen.custom_curve = Some(arr);
            points = CurvePoints {
                p1: arr[0],
                p2: arr[1],
            };
            app.mark_dirty();
        }
    }
    if response.drag_stopped() {
        app.curve_drag = None;
    }

    ui.horizontal(|ui| {
        if ui
            .button(if custom {
                "Reset curve"
            } else {
                "Edit control points"
            })
            .clicked()
        {
            if custom {
                cfg.pen.custom_curve = None;
            } else {
                cfg.pen.custom_curve = Some([points.p1, points.p2]);
            }
            app.mark_dirty();
        }
        ui.label(
            RichText::new("drag P1 / P2 in the graph")
                .small()
                .color(muted()),
        );
    });
}
