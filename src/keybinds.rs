use std::collections::HashMap;

use iced::keyboard::{
    self,
    key::{self, Physical},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Next,
    Previous,
    ToggleFullscreen,
    FocusScale,
    PasteFromClipboard,
    ZoomIn,
    ZoomOut,
    ZoomFit,
    ZoomPreset(u8),
    UiScaleUp,
    UiScaleDown,
    UiScaleReset,
    RotateCw,
    RotateCcw,
}

impl Action {
    pub fn label_with_detail(&self) -> String {
        match self {
            Self::Next => "Next image".into(),
            Self::Previous => "Previous image".into(),
            Self::ToggleFullscreen => "Toggle fullscreen".into(),
            Self::FocusScale => "Focus zoom entry".into(),
            Self::PasteFromClipboard => "Paste from clipboard".into(),
            Self::ZoomIn => "Zoom in".into(),
            Self::ZoomOut => "Zoom out".into(),
            Self::ZoomFit => "Fit to viewport".into(),
            Self::ZoomPreset(n) => format!("Zoom {}×", n),
            Self::UiScaleUp => "UI scale up".into(),
            Self::UiScaleDown => "UI scale down".into(),
            Self::UiScaleReset => "UI scale reset".into(),
            Self::RotateCw => "Rotate clockwise".into(),
            Self::RotateCcw => "Rotate counter-clockwise".into(),
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Next => "Go to the next file in the folder",
            Self::Previous => "Go to the previous file in the folder",
            Self::ToggleFullscreen => "Switch between windowed and fullscreen mode",
            Self::FocusScale => "Focus the zoom percentage entry field",
            Self::PasteFromClipboard => "Load an image from the clipboard",
            Self::ZoomIn => "Increase zoom level",
            Self::ZoomOut => "Decrease zoom level",
            Self::ZoomFit => "Fit the image to the viewport",
            Self::ZoomPreset(_) => "Jump to a fixed zoom multiplier",
            Self::UiScaleUp => "Increase the application UI scale",
            Self::UiScaleDown => "Decrease the application UI scale",
            Self::UiScaleReset => "Reset the application UI scale to 100%",
            Self::RotateCw => "Rotate the image 90° clockwise",
            Self::RotateCcw => "Rotate the image 90° counter-clockwise",
        }
    }

    pub fn all_visible() -> &'static [Action] {
        &[
            Action::Next,
            Action::Previous,
            Action::ToggleFullscreen,
            Action::PasteFromClipboard,
            Action::RotateCw,
            Action::RotateCcw,
            Action::ZoomIn,
            Action::ZoomOut,
            Action::ZoomFit,
            Action::FocusScale,
            Action::UiScaleUp,
            Action::UiScaleDown,
            Action::UiScaleReset,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyBinding {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub code: key::Code,
}

impl KeyBinding {
    pub fn matches(&self, physical_key: &Physical, modifiers: &keyboard::Modifiers) -> bool {
        if let Physical::Code(code) = physical_key {
            code == &self.code
                && modifiers.control() == self.ctrl
                && modifiers.shift() == self.shift
                && modifiers.alt() == self.alt
        } else {
            false
        }
    }

    pub fn display(&self) -> String {
        let mut s = String::new();
        if self.ctrl {
            s.push_str("Ctrl+");
        }
        if self.shift {
            s.push_str("Shift+");
        }
        if self.alt {
            s.push_str("Alt+");
        }
        s.push_str(code_name(self.code));
        s
    }

    pub fn from_str(s: &str) -> Option<Self> {
        let mut ctrl = false;
        let mut shift = false;
        let mut alt = false;
        let mut parts = s.split('+').peekable();
        loop {
            match parts.peek() {
                Some(&"Ctrl") => {
                    ctrl = true;
                    parts.next();
                }
                Some(&"Shift") => {
                    shift = true;
                    parts.next();
                }
                Some(&"Alt") => {
                    alt = true;
                    parts.next();
                }
                _ => break,
            }
        }
        let code = name_to_code(parts.next()?)?;
        Some(Self {
            ctrl,
            shift,
            alt,
            code,
        })
    }
}

const CODE_NAMES: &[(key::Code, &str)] = &[
    (key::Code::ArrowRight, "ArrowRight"),
    (key::Code::ArrowLeft, "ArrowLeft"),
    (key::Code::ArrowUp, "ArrowUp"),
    (key::Code::ArrowDown, "ArrowDown"),
    (key::Code::Equal, "Equal"),
    (key::Code::Minus, "Minus"),
    (key::Code::Digit0, "0"),
    (key::Code::Digit1, "1"),
    (key::Code::Digit2, "2"),
    (key::Code::Digit3, "3"),
    (key::Code::Digit4, "4"),
    (key::Code::Digit5, "5"),
    (key::Code::Digit6, "6"),
    (key::Code::Digit7, "7"),
    (key::Code::Digit8, "8"),
    (key::Code::Digit9, "9"),
    (key::Code::KeyA, "A"),
    (key::Code::KeyB, "B"),
    (key::Code::KeyC, "C"),
    (key::Code::KeyD, "D"),
    (key::Code::KeyE, "E"),
    (key::Code::KeyF, "F"),
    (key::Code::KeyG, "G"),
    (key::Code::KeyH, "H"),
    (key::Code::KeyI, "I"),
    (key::Code::KeyJ, "J"),
    (key::Code::KeyK, "K"),
    (key::Code::KeyL, "L"),
    (key::Code::KeyM, "M"),
    (key::Code::KeyN, "N"),
    (key::Code::KeyO, "O"),
    (key::Code::KeyP, "P"),
    (key::Code::KeyQ, "Q"),
    (key::Code::KeyR, "R"),
    (key::Code::KeyS, "S"),
    (key::Code::KeyT, "T"),
    (key::Code::KeyU, "U"),
    (key::Code::KeyV, "V"),
    (key::Code::KeyW, "W"),
    (key::Code::KeyX, "X"),
    (key::Code::KeyY, "Y"),
    (key::Code::KeyZ, "Z"),
    (key::Code::Space, "Space"),
    (key::Code::Enter, "Enter"),
    (key::Code::Escape, "Escape"),
    (key::Code::Backspace, "Backspace"),
    (key::Code::Tab, "Tab"),
    (key::Code::Delete, "Delete"),
    (key::Code::Home, "Home"),
    (key::Code::End, "End"),
    (key::Code::PageUp, "PageUp"),
    (key::Code::PageDown, "PageDown"),
    (key::Code::F1, "F1"),
    (key::Code::F2, "F2"),
    (key::Code::F3, "F3"),
    (key::Code::F4, "F4"),
    (key::Code::F5, "F5"),
    (key::Code::F6, "F6"),
    (key::Code::F7, "F7"),
    (key::Code::F8, "F8"),
    (key::Code::F9, "F9"),
    (key::Code::F10, "F10"),
    (key::Code::F11, "F11"),
    (key::Code::F12, "F12"),
    (key::Code::BracketLeft, "BracketLeft"),
    (key::Code::BracketRight, "BracketRight"),
    (key::Code::Backslash, "Backslash"),
    (key::Code::Semicolon, "Semicolon"),
    (key::Code::Quote, "Quote"),
    (key::Code::Comma, "Comma"),
    (key::Code::Period, "Period"),
    (key::Code::Slash, "Slash"),
    (key::Code::Backquote, "Backquote"),
];

fn code_name(code: key::Code) -> &'static str {
    CODE_NAMES
        .iter()
        .find(|(c, _)| *c == code)
        .map_or("Unknown", |(_, n)| n)
}

fn name_to_code(s: &str) -> Option<key::Code> {
    CODE_NAMES.iter().find(|(_, n)| *n == s).map(|(c, _)| *c)
}

#[derive(Debug, Clone)]
pub struct Keymap {
    bindings: HashMap<Action, KeyBinding>,
}

impl Default for Keymap {
    fn default() -> Self {
        let c = |code| KeyBinding {
            ctrl: true,
            shift: false,
            alt: false,
            code,
        };
        let n = |code| KeyBinding {
            ctrl: false,
            shift: false,
            alt: false,
            code,
        };
        let mut m = HashMap::new();
        m.insert(Action::Next, n(key::Code::ArrowRight));
        m.insert(Action::Previous, n(key::Code::ArrowLeft));
        m.insert(Action::ToggleFullscreen, n(key::Code::KeyF));
        m.insert(Action::FocusScale, n(key::Code::KeyZ));
        m.insert(Action::PasteFromClipboard, c(key::Code::KeyV));
        m.insert(Action::ZoomIn, c(key::Code::Equal));
        m.insert(Action::ZoomOut, c(key::Code::Minus));
        m.insert(Action::ZoomFit, c(key::Code::Digit0));
        m.insert(Action::UiScaleUp, n(key::Code::Equal));
        m.insert(Action::UiScaleDown, n(key::Code::Minus));
        m.insert(Action::UiScaleReset, n(key::Code::Digit0));
        m.insert(Action::RotateCw, n(key::Code::BracketRight));
        m.insert(Action::RotateCcw, n(key::Code::BracketLeft));
        let digit_codes = [
            key::Code::Digit1,
            key::Code::Digit2,
            key::Code::Digit3,
            key::Code::Digit4,
            key::Code::Digit5,
            key::Code::Digit6,
            key::Code::Digit7,
            key::Code::Digit8,
            key::Code::Digit9,
        ];
        for (i, code) in digit_codes.into_iter().enumerate() {
            m.insert(Action::ZoomPreset(i as u8 + 1), c(code));
        }
        Self { bindings: m }
    }
}

impl Keymap {
    pub fn resolve(
        &self,
        physical_key: &Physical,
        modifiers: &keyboard::Modifiers,
    ) -> Option<Action> {
        self.bindings
            .iter()
            .find(|(_, b)| b.matches(physical_key, modifiers))
            .map(|(a, _)| *a)
    }

    pub fn binding_for(&self, action: &Action) -> Option<&KeyBinding> {
        self.bindings.get(action)
    }

    pub fn set(&mut self, action: Action, binding: KeyBinding) {
        self.bindings.retain(|a, b| *b != binding || *a == action);
        self.bindings.insert(action, binding);
    }

    pub fn remove(&mut self, action: &Action) {
        self.bindings.remove(action);
    }
}

#[derive(Serialize, Deserialize, Default)]
pub(crate) struct KeymapFile {
    pub next: Option<String>,
    pub previous: Option<String>,
    pub toggle_fullscreen: Option<String>,
    pub focus_scale: Option<String>,
    pub paste_from_clipboard: Option<String>,
    pub zoom_in: Option<String>,
    pub zoom_out: Option<String>,
    pub zoom_fit: Option<String>,
    pub zoom_preset_1: Option<String>,
    pub zoom_preset_2: Option<String>,
    pub zoom_preset_3: Option<String>,
    pub zoom_preset_4: Option<String>,
    pub zoom_preset_5: Option<String>,
    pub zoom_preset_6: Option<String>,
    pub zoom_preset_7: Option<String>,
    pub zoom_preset_8: Option<String>,
    pub zoom_preset_9: Option<String>,
    pub ui_scale_up: Option<String>,
    pub ui_scale_down: Option<String>,
    pub ui_scale_reset: Option<String>,
    pub rotate_cw: Option<String>,
    pub rotate_ccw: Option<String>,
}

impl From<&Keymap> for KeymapFile {
    fn from(km: &Keymap) -> Self {
        let bind = |a: Action| {
            Some(
                km.bindings
                    .get(&a)
                    .map(|kb| kb.display())
                    .unwrap_or_default(),
            )
        };
        Self {
            next: bind(Action::Next),
            previous: bind(Action::Previous),
            toggle_fullscreen: bind(Action::ToggleFullscreen),
            focus_scale: bind(Action::FocusScale),
            paste_from_clipboard: bind(Action::PasteFromClipboard),
            zoom_in: bind(Action::ZoomIn),
            zoom_out: bind(Action::ZoomOut),
            zoom_fit: bind(Action::ZoomFit),
            zoom_preset_1: bind(Action::ZoomPreset(1)),
            zoom_preset_2: bind(Action::ZoomPreset(2)),
            zoom_preset_3: bind(Action::ZoomPreset(3)),
            zoom_preset_4: bind(Action::ZoomPreset(4)),
            zoom_preset_5: bind(Action::ZoomPreset(5)),
            zoom_preset_6: bind(Action::ZoomPreset(6)),
            zoom_preset_7: bind(Action::ZoomPreset(7)),
            zoom_preset_8: bind(Action::ZoomPreset(8)),
            zoom_preset_9: bind(Action::ZoomPreset(9)),
            ui_scale_up: bind(Action::UiScaleUp),
            ui_scale_down: bind(Action::UiScaleDown),
            ui_scale_reset: bind(Action::UiScaleReset),
            rotate_cw: bind(Action::RotateCw),
            rotate_ccw: bind(Action::RotateCcw),
        }
    }
}

impl From<KeymapFile> for Keymap {
    fn from(f: KeymapFile) -> Self {
        let defaults = Keymap::default();
        let resolve = |raw: Option<String>, action: Action| -> Option<(Action, KeyBinding)> {
            match raw {
                None => defaults.bindings.get(&action).map(|b| (action, *b)),
                Some(ref s) if s.is_empty() => None,
                Some(ref s) => KeyBinding::from_str(s).map(|b| (action, b)),
            }
        };
        let bindings = [
            resolve(f.next, Action::Next),
            resolve(f.previous, Action::Previous),
            resolve(f.toggle_fullscreen, Action::ToggleFullscreen),
            resolve(f.focus_scale, Action::FocusScale),
            resolve(f.paste_from_clipboard, Action::PasteFromClipboard),
            resolve(f.zoom_in, Action::ZoomIn),
            resolve(f.zoom_out, Action::ZoomOut),
            resolve(f.zoom_fit, Action::ZoomFit),
            resolve(f.zoom_preset_1, Action::ZoomPreset(1)),
            resolve(f.zoom_preset_2, Action::ZoomPreset(2)),
            resolve(f.zoom_preset_3, Action::ZoomPreset(3)),
            resolve(f.zoom_preset_4, Action::ZoomPreset(4)),
            resolve(f.zoom_preset_5, Action::ZoomPreset(5)),
            resolve(f.zoom_preset_6, Action::ZoomPreset(6)),
            resolve(f.zoom_preset_7, Action::ZoomPreset(7)),
            resolve(f.zoom_preset_8, Action::ZoomPreset(8)),
            resolve(f.zoom_preset_9, Action::ZoomPreset(9)),
            resolve(f.ui_scale_up, Action::UiScaleUp),
            resolve(f.ui_scale_down, Action::UiScaleDown),
            resolve(f.ui_scale_reset, Action::UiScaleReset),
            resolve(f.rotate_cw, Action::RotateCw),
            resolve(f.rotate_ccw, Action::RotateCcw),
        ]
        .into_iter()
        .flatten()
        .collect();
        Self { bindings }
    }
}
