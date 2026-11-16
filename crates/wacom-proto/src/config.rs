//! Persistent configuration shared by daemon, CLI and GUI.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::transform::{NormRect, Orientation};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modifier {
    Ctrl,
    Shift,
    Alt,
    Super,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseAction {
    Left,
    Right,
    Middle,
    DoubleClick,
    ScrollUp,
    ScrollDown,
    PanScroll,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Action {
    #[default]
    Disabled,
    Key {
        #[serde(default)]
        mods: Vec<Modifier>,
        key: String,
    },
    Mouse(MouseAction),
    Precision,
    ToggleMonitor,
    RadialMenu,
    Shell {
        command: String,
    },
}

impl Action {
    pub fn key(mods: &[Modifier], key: &str) -> Self {
        Action::Key {
            mods: mods.to_vec(),
            key: key.to_string(),
        }
    }

    /// Short human label used by the GUI and CLI.
    pub fn label(&self) -> String {
        match self {
            Action::Disabled => "Disabled".into(),
            Action::Key { mods, key } => {
                let mut parts: Vec<String> = mods.iter().map(|m| format!("{m:?}")).collect();
                parts.push(key.clone());
                parts.join("+")
            }
            Action::Mouse(m) => format!("Mouse: {m:?}"),
            Action::Precision => "Precision mode".into(),
            Action::ToggleMonitor => "Toggle monitor".into(),
            Action::RadialMenu => "Radial menu".into(),
            Action::Shell { command } => format!("Shell: {command}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PenConfig {
    pub button1: Action,
    pub button2: Action,
    /// 0.0 = soft, 0.5 = linear, 1.0 = firm.
    pub tip_feel: f32,
    /// Optional custom cubic Bezier control points (p1, p2).
    pub custom_curve: Option<[[f32; 2]; 2]>,
    /// Screen pixels within which a second tap is snapped to the first one.
    pub double_click_distance: f32,
    pub double_click_time_ms: u64,
}

impl Default for PenConfig {
    fn default() -> Self {
        Self {
            // Disabled = pass the native BTN_STYLUS / BTN_STYLUS2 through.
            button1: Action::Disabled,
            button2: Action::Disabled,
            tip_feel: 0.5,
            custom_curve: None,
            double_click_distance: 10.0,
            double_click_time_ms: 400,
        }
    }
}

impl PenConfig {
    pub fn curve(&self) -> crate::pressure::PressureCurve {
        match self.custom_curve {
            Some([p1, p2]) => {
                crate::pressure::PressureCurve::new(crate::pressure::CurvePoints { p1, p2 })
            }
            None => crate::pressure::PressureCurve::from_tip_feel(self.tip_feel),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "mode", content = "name", rename_all = "snake_case")]
pub enum ScreenSelection {
    #[default]
    All,
    Monitor(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MappingConfig {
    pub screen: ScreenSelection,
    pub tablet_area: NormRect,
    pub force_proportions: bool,
    pub orientation: Orientation,
}

impl Default for MappingConfig {
    fn default() -> Self {
        Self {
            screen: ScreenSelection::All,
            tablet_area: NormRect::FULL,
            force_proportions: true,
            orientation: Orientation::Deg0,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub pen: Option<PenConfig>,
    pub expresskeys: Option<[Action; 4]>,
    pub mapping: Option<MappingConfig>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub version: u32,
    pub pen: PenConfig,
    pub expresskeys: [Action; 4],
    pub mapping: MappingConfig,
    /// Per-application overrides, keyed by Hyprland `class` (`app_id`).
    pub profiles: BTreeMap<String, Profile>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            pen: PenConfig::default(),
            expresskeys: [
                Action::key(&[Modifier::Ctrl], "z"),
                Action::key(&[Modifier::Ctrl, Modifier::Shift], "z"),
                Action::key(&[Modifier::Ctrl], "minus"),
                Action::key(&[Modifier::Ctrl, Modifier::Shift], "equal"),
            ],
            mapping: MappingConfig::default(),
            profiles: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectiveConfig {
    pub pen: PenConfig,
    pub expresskeys: [Action; 4],
    pub mapping: MappingConfig,
}

impl Config {
    /// Merge the matching application profile over the global config.
    /// Exact class match wins; otherwise a case-insensitive match is used.
    pub fn effective(&self, app_class: Option<&str>) -> EffectiveConfig {
        let mut pen = self.pen.clone();
        let mut keys = self.expresskeys.clone();
        let mut mapping = self.mapping.clone();
        if let Some(profile) = app_class.and_then(|class| self.profile_for(class)) {
            if let Some(p) = &profile.pen {
                pen = p.clone();
            }
            if let Some(k) = &profile.expresskeys {
                keys = k.clone();
            }
            if let Some(m) = &profile.mapping {
                mapping = m.clone();
            }
        }
        EffectiveConfig {
            pen,
            expresskeys: keys,
            mapping,
        }
    }

    pub fn profile_for(&self, app_class: &str) -> Option<&Profile> {
        if let Some(p) = self.profiles.get(app_class) {
            return Some(p);
        }
        self.profiles
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(app_class))
            .map(|(_, v)| v)
    }
}

pub fn default_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("wacom-ctl4100").join("config.json")
}

pub fn load(path: &Path) -> io::Result<Config> {
    match fs::read_to_string(path) {
        Ok(text) => {
            serde_json::from_str(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(e),
    }
}

/// Write atomically (temp file + rename) so a crash cannot truncate the config.
pub fn save(path: &Path, config: &Config) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(config)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text + "\n")?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip() {
        let c = Config::default();
        let json = serde_json::to_string(&c).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn profile_merges_only_set_fields() {
        let mut c = Config::default();
        c.profiles.insert(
            "krita".into(),
            Profile {
                pen: Some(PenConfig {
                    tip_feel: 0.1,
                    ..PenConfig::default()
                }),
                ..Profile::default()
            },
        );
        let eff = c.effective(Some("krita"));
        assert_eq!(eff.pen.tip_feel, 0.1);
        assert_eq!(eff.expresskeys, c.expresskeys);
        assert_eq!(eff.mapping, c.mapping);

        let global = c.effective(Some("kitty"));
        assert_eq!(global.pen, c.pen);
    }

    #[test]
    fn profile_match_is_case_insensitive() {
        let mut c = Config::default();
        c.profiles.insert("Krita".into(), Profile::default());
        assert_eq!(c.effective(Some("krita")).pen, PenConfig::default());
    }

    #[test]
    fn action_json_is_stable() {
        let a = Action::key(&[Modifier::Ctrl, Modifier::Shift], "z");
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(
            json,
            r#"{"type":"key","value":{"mods":["ctrl","shift"],"key":"z"}}"#
        );
        let back: Action = serde_json::from_str(&json).unwrap();
        assert_eq!(a, back);
    }

    #[test]
    fn save_and_load() {
        let dir = std::env::temp_dir().join(format!("wacom-proto-test-{}", std::process::id()));
        let path = dir.join("config.json");
        let mut c = Config::default();
        c.pen.tip_feel = 0.25;
        save(&path, &c).unwrap();
        assert_eq!(load(&path).unwrap(), c);
        std::fs::remove_dir_all(dir).ok();
    }
}
