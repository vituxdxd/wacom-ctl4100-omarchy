//! Hyprland integration: monitor geometry and focused application class.

use std::io::{BufRead, BufReader};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use serde::Deserialize;
use wacom_proto::ipc::Monitor;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HyprEvent {
    AppChanged(Option<String>),
    MonitorsChanged,
}

#[derive(Debug, Deserialize)]
struct RawMonitor {
    name: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    #[serde(default = "one")]
    scale: f64,
    #[serde(default)]
    focused: bool,
}

fn one() -> f64 {
    1.0
}

#[derive(Debug, Deserialize)]
struct RawWindow {
    #[serde(default)]
    class: String,
}

pub fn parse_monitors(json: &str) -> Option<Vec<Monitor>> {
    let raw: Vec<RawMonitor> = serde_json::from_str(json).ok()?;
    Some(
        raw.into_iter()
            .map(|m| {
                let scale = if m.scale > 0.0 { m.scale } else { 1.0 };
                Monitor {
                    name: m.name,
                    // Hyprland reports physical mode size; layout uses logical px.
                    x: m.x,
                    y: m.y,
                    width: m.width / scale,
                    height: m.height / scale,
                    scale,
                    focused: m.focused,
                }
            })
            .collect(),
    )
}

pub fn parse_activewindow(json: &str) -> Option<String> {
    let w: RawWindow = serde_json::from_str(json).ok()?;
    if w.class.is_empty() {
        None
    } else {
        Some(w.class)
    }
}

pub fn parse_event_line(line: &str) -> Option<HyprEvent> {
    if let Some(rest) = line.strip_prefix("activewindow>>") {
        let class = rest.split(',').next().unwrap_or("").trim();
        return Some(HyprEvent::AppChanged(if class.is_empty() {
            None
        } else {
            Some(class.to_string())
        }));
    }
    if line.starts_with("monitoradded>>")
        || line.starts_with("monitorremoved>>")
        || line.starts_with("focusedmon>>")
    {
        return Some(HyprEvent::MonitorsChanged);
    }
    None
}

fn hyprctl(args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("hyprctl")
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

pub fn monitors() -> Option<Vec<Monitor>> {
    parse_monitors(&hyprctl(&["monitors", "-j"])?)
}

pub fn active_class() -> Option<String> {
    parse_activewindow(&hyprctl(&["activewindow", "-j"])?)
}

/// When the user service starts without Hyprland's environment, discover the
/// newest instance under `$XDG_RUNTIME_DIR/hypr` so `hyprctl` works.
pub fn ensure_instance_env() {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        return;
    }
    let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(PathBuf::from(runtime).join("hypr")) else {
        return;
    };
    let mut newest: Option<(std::time::SystemTime, String)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        let socket = path.join(".socket2.sock");
        let Ok(meta) = std::fs::metadata(&socket) else {
            continue;
        };
        let Ok(modified) = meta.modified() else {
            continue;
        };
        if newest.as_ref().map(|(t, _)| modified > *t).unwrap_or(true) {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                newest = Some((modified, name.to_string()));
            }
        }
    }
    if let Some((_, signature)) = newest {
        std::env::set_var("HYPRLAND_INSTANCE_SIGNATURE", signature);
    }
}

fn socket2_path() -> Option<PathBuf> {
    let sig = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")?;
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")?;
    Some(
        PathBuf::from(runtime)
            .join("hypr")
            .join(sig)
            .join(".socket2.sock"),
    )
}

/// Listen for focus/monitor changes; reconnects if Hyprland restarts.
pub fn spawn_listener(tx: Sender<HyprEvent>) {
    thread::spawn(move || loop {
        if let Some(path) = socket2_path() {
            if let Ok(stream) = UnixStream::connect(&path) {
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            if let Some(ev) = parse_event_line(&line) {
                                if tx.send(ev).is_err() {
                                    return;
                                }
                            }
                        }
                    }
                }
            }
        }
        thread::sleep(Duration::from_secs(2));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scaled_monitors() {
        let json = r#"[
            {"name":"eDP-1","x":0,"y":0,"width":3840,"height":2160,"scale":2.0,"focused":true},
            {"name":"DP-1","x":1920,"y":0,"width":2560,"height":1440,"scale":1.0}
        ]"#;
        let m = parse_monitors(json).unwrap();
        assert_eq!(m[0].width, 1920.0);
        assert_eq!(m[0].height, 1080.0);
        assert_eq!(m[1].x, 1920.0);
        assert!(m[0].focused);
        assert!(!m[1].focused);
    }

    #[test]
    fn parses_active_window() {
        assert_eq!(
            parse_activewindow(r#"{"class":"krita","title":"x"}"#),
            Some("krita".into())
        );
        assert_eq!(parse_activewindow(r#"{"class":""}"#), None);
        assert_eq!(parse_activewindow("not json"), None);
    }

    #[test]
    fn parses_events() {
        assert_eq!(
            parse_event_line("activewindow>>kitty,~"),
            Some(HyprEvent::AppChanged(Some("kitty".into())))
        );
        assert_eq!(
            parse_event_line("activewindow>>,~"),
            Some(HyprEvent::AppChanged(None))
        );
        assert_eq!(
            parse_event_line("monitorremoved>>DP-1"),
            Some(HyprEvent::MonitorsChanged)
        );
        assert_eq!(parse_event_line("workspace>>2"), None);
    }
}
