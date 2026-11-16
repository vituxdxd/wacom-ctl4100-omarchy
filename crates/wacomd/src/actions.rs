//! Execute mapped actions on a virtual keyboard/mouse uinput device.

use std::io;
use std::thread;
use std::time::Duration;

use evdev::uinput::VirtualDevice;
use evdev::{AttributeSet, BusType, EventType, InputEvent, InputId, KeyCode, RelativeAxisCode};
use wacom_proto::config::{Action, Modifier, MouseAction};

const CLICK_HOLD: Duration = Duration::from_millis(12);
const DOUBLE_CLICK_GAP: Duration = Duration::from_millis(45);

const KEY_NAMES: &[(&str, KeyCode)] = &[
    ("a", KeyCode::KEY_A),
    ("b", KeyCode::KEY_B),
    ("c", KeyCode::KEY_C),
    ("d", KeyCode::KEY_D),
    ("e", KeyCode::KEY_E),
    ("f", KeyCode::KEY_F),
    ("g", KeyCode::KEY_G),
    ("h", KeyCode::KEY_H),
    ("i", KeyCode::KEY_I),
    ("j", KeyCode::KEY_J),
    ("k", KeyCode::KEY_K),
    ("l", KeyCode::KEY_L),
    ("m", KeyCode::KEY_M),
    ("n", KeyCode::KEY_N),
    ("o", KeyCode::KEY_O),
    ("p", KeyCode::KEY_P),
    ("q", KeyCode::KEY_Q),
    ("r", KeyCode::KEY_R),
    ("s", KeyCode::KEY_S),
    ("t", KeyCode::KEY_T),
    ("u", KeyCode::KEY_U),
    ("v", KeyCode::KEY_V),
    ("w", KeyCode::KEY_W),
    ("x", KeyCode::KEY_X),
    ("y", KeyCode::KEY_Y),
    ("z", KeyCode::KEY_Z),
    ("0", KeyCode::KEY_0),
    ("1", KeyCode::KEY_1),
    ("2", KeyCode::KEY_2),
    ("3", KeyCode::KEY_3),
    ("4", KeyCode::KEY_4),
    ("5", KeyCode::KEY_5),
    ("6", KeyCode::KEY_6),
    ("7", KeyCode::KEY_7),
    ("8", KeyCode::KEY_8),
    ("9", KeyCode::KEY_9),
    ("f1", KeyCode::KEY_F1),
    ("f2", KeyCode::KEY_F2),
    ("f3", KeyCode::KEY_F3),
    ("f4", KeyCode::KEY_F4),
    ("f5", KeyCode::KEY_F5),
    ("f6", KeyCode::KEY_F6),
    ("f7", KeyCode::KEY_F7),
    ("f8", KeyCode::KEY_F8),
    ("f9", KeyCode::KEY_F9),
    ("f10", KeyCode::KEY_F10),
    ("f11", KeyCode::KEY_F11),
    ("f12", KeyCode::KEY_F12),
    ("left", KeyCode::KEY_LEFT),
    ("right", KeyCode::KEY_RIGHT),
    ("up", KeyCode::KEY_UP),
    ("down", KeyCode::KEY_DOWN),
    ("enter", KeyCode::KEY_ENTER),
    ("return", KeyCode::KEY_ENTER),
    ("space", KeyCode::KEY_SPACE),
    ("tab", KeyCode::KEY_TAB),
    ("escape", KeyCode::KEY_ESC),
    ("esc", KeyCode::KEY_ESC),
    ("backspace", KeyCode::KEY_BACKSPACE),
    ("delete", KeyCode::KEY_DELETE),
    ("insert", KeyCode::KEY_INSERT),
    ("home", KeyCode::KEY_HOME),
    ("end", KeyCode::KEY_END),
    ("pageup", KeyCode::KEY_PAGEUP),
    ("pagedown", KeyCode::KEY_PAGEDOWN),
    ("minus", KeyCode::KEY_MINUS),
    ("equal", KeyCode::KEY_EQUAL),
    ("plus", KeyCode::KEY_EQUAL),
    ("comma", KeyCode::KEY_COMMA),
    ("period", KeyCode::KEY_DOT),
    ("slash", KeyCode::KEY_SLASH),
    ("backslash", KeyCode::KEY_BACKSLASH),
    ("semicolon", KeyCode::KEY_SEMICOLON),
    ("apostrophe", KeyCode::KEY_APOSTROPHE),
    ("grave", KeyCode::KEY_GRAVE),
    ("leftbrace", KeyCode::KEY_LEFTBRACE),
    ("rightbrace", KeyCode::KEY_RIGHTBRACE),
    ("printscreen", KeyCode::KEY_SYSRQ),
];

pub fn key_code(name: &str) -> Option<KeyCode> {
    let lower = name.to_ascii_lowercase();
    KEY_NAMES.iter().find(|(n, _)| *n == lower).map(|(_, k)| *k)
}

fn modifier_code(m: Modifier) -> KeyCode {
    match m {
        Modifier::Ctrl => KeyCode::KEY_LEFTCTRL,
        Modifier::Shift => KeyCode::KEY_LEFTSHIFT,
        Modifier::Alt => KeyCode::KEY_LEFTALT,
        Modifier::Super => KeyCode::KEY_LEFTMETA,
    }
}

pub struct ActionRunner {
    device: VirtualDevice,
}

impl ActionRunner {
    pub fn new() -> io::Result<Self> {
        let mut keys = AttributeSet::<KeyCode>::new();
        for (_, key) in KEY_NAMES {
            keys.insert(*key);
        }
        for key in [
            KeyCode::BTN_LEFT,
            KeyCode::BTN_RIGHT,
            KeyCode::BTN_MIDDLE,
            KeyCode::KEY_LEFTCTRL,
            KeyCode::KEY_LEFTSHIFT,
            KeyCode::KEY_LEFTALT,
            KeyCode::KEY_LEFTMETA,
        ] {
            keys.insert(key);
        }
        let mut rel = AttributeSet::<RelativeAxisCode>::new();
        rel.insert(RelativeAxisCode::REL_WHEEL);
        rel.insert(RelativeAxisCode::REL_HWHEEL);

        let device = VirtualDevice::builder()?
            .name("Wacom Intuos S (CTL-4100) Pad")
            .input_id(InputId::new(BusType::BUS_VIRTUAL, 0x056a, 0x0374, 1))
            .with_keys(&keys)?
            .with_relative_axes(&rel)?
            .build()?;
        Ok(Self { device })
    }

    fn write(&mut self, events: &[InputEvent]) {
        if let Err(e) = self.device.emit(events) {
            eprintln!("wacomd: pad emit failed: {e}");
        }
    }

    pub fn tap_key(&mut self, mods: &[Modifier], name: &str) {
        let Some(key) = key_code(name) else {
            eprintln!("wacomd: unknown key name {name:?}");
            return;
        };
        let mod_codes: Vec<KeyCode> = mods.iter().copied().map(modifier_code).collect();
        let mut down: Vec<InputEvent> = mod_codes
            .iter()
            .map(|k| InputEvent::new(EventType::KEY.0, k.0, 1))
            .collect();
        down.push(InputEvent::new(EventType::KEY.0, key.0, 1));
        self.write(&down);
        thread::sleep(CLICK_HOLD);
        let mut up: Vec<InputEvent> = vec![InputEvent::new(EventType::KEY.0, key.0, 0)];
        up.extend(
            mod_codes
                .iter()
                .rev()
                .map(|k| InputEvent::new(EventType::KEY.0, k.0, 0)),
        );
        self.write(&up);
    }

    pub fn click(&mut self, button: KeyCode, count: u8) {
        for i in 0..count.max(1) {
            if i > 0 {
                thread::sleep(DOUBLE_CLICK_GAP);
            }
            self.write(&[InputEvent::new(EventType::KEY.0, button.0, 1)]);
            thread::sleep(CLICK_HOLD);
            self.write(&[InputEvent::new(EventType::KEY.0, button.0, 0)]);
        }
    }

    pub fn set_button(&mut self, button: KeyCode, down: bool) {
        self.write(&[InputEvent::new(EventType::KEY.0, button.0, i32::from(down))]);
    }

    pub fn wheel(&mut self, vertical: i32, horizontal: i32) {
        let mut events = Vec::with_capacity(2);
        if vertical != 0 {
            events.push(InputEvent::new(
                EventType::RELATIVE.0,
                RelativeAxisCode::REL_WHEEL.0,
                vertical,
            ));
        }
        if horizontal != 0 {
            events.push(InputEvent::new(
                EventType::RELATIVE.0,
                RelativeAxisCode::REL_HWHEEL.0,
                horizontal,
            ));
        }
        if !events.is_empty() {
            self.write(&events);
        }
    }
}

/// Run actions that do not need daemon state. Internal actions return `false`
/// so the caller can handle them (precision, monitor switch, radial menu).
pub fn run_external(runner: &mut ActionRunner, action: &Action) -> bool {
    match action {
        Action::Key { mods, key } => runner.tap_key(mods, key),
        Action::Mouse(MouseAction::Left) => runner.click(KeyCode::BTN_LEFT, 1),
        Action::Mouse(MouseAction::Right) => runner.click(KeyCode::BTN_RIGHT, 1),
        Action::Mouse(MouseAction::Middle) => runner.click(KeyCode::BTN_MIDDLE, 1),
        Action::Mouse(MouseAction::DoubleClick) => runner.click(KeyCode::BTN_LEFT, 2),
        Action::Mouse(MouseAction::ScrollUp) => runner.wheel(1, 0),
        Action::Mouse(MouseAction::ScrollDown) => runner.wheel(-1, 0),
        Action::Shell { command } => {
            match std::process::Command::new("sh")
                .arg("-c")
                .arg(command)
                .spawn()
            {
                Ok(mut child) => {
                    thread::spawn(move || {
                        let _ = child.wait();
                    });
                }
                Err(e) => eprintln!("wacomd: shell action failed: {e}"),
            }
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_common_keys() {
        assert_eq!(key_code("Z"), Some(KeyCode::KEY_Z));
        assert_eq!(key_code("F11"), Some(KeyCode::KEY_F11));
        assert_eq!(key_code("plus"), Some(KeyCode::KEY_EQUAL));
        assert_eq!(key_code("nope"), None);
    }

    #[test]
    fn modifiers_are_distinct() {
        assert_eq!(modifier_code(Modifier::Super), KeyCode::KEY_LEFTMETA);
    }
}
