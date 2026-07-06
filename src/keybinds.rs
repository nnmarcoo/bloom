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
    ToolSelect,
    ToolCrop,
    ToolDraw,
    ToolText,
    BrushSizeUp,
    BrushSizeDown,
    TogglePlayback,
    FrameFirst,
    FrameLast,
    FrameNext,
    FramePrev,
    ToggleMute,
    ToggleInfoPanel,
    ToggleEditPanel,
    ToggleCheckerboard,
    ToggleBottomBar,
    OpenMedia,
    CopyImage,
    ExportImage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCategory {
    Navigation,
    View,
    Tools,
    Playback,
}

impl KeyCategory {
    pub fn all() -> &'static [KeyCategory] {
        &[
            KeyCategory::Navigation,
            KeyCategory::View,
            KeyCategory::Tools,
            KeyCategory::Playback,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            KeyCategory::Navigation => "Navigation",
            KeyCategory::View => "View & Zoom",
            KeyCategory::Tools => "Tools",
            KeyCategory::Playback => "Playback",
        }
    }
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
            Self::ToolSelect => "Select tool".into(),
            Self::ToolCrop => "Crop tool".into(),
            Self::ToolDraw => "Draw tool".into(),
            Self::ToolText => "Text tool".into(),
            Self::BrushSizeUp => "Brush size up".into(),
            Self::BrushSizeDown => "Brush size down".into(),
            Self::TogglePlayback => "Toggle playback".into(),
            Self::FrameFirst => "First frame".into(),
            Self::FrameLast => "Last frame".into(),
            Self::FrameNext => "Next frame".into(),
            Self::FramePrev => "Previous frame".into(),
            Self::ToggleMute => "Toggle mute".into(),
            Self::ToggleInfoPanel => "Toggle info panel".into(),
            Self::ToggleEditPanel => "Toggle edit panel".into(),
            Self::ToggleCheckerboard => "Toggle checkerboard".into(),
            Self::ToggleBottomBar => "Toggle bottom bar".into(),
            Self::OpenMedia => "Open media".into(),
            Self::CopyImage => "Copy image".into(),
            Self::ExportImage => "Export image".into(),
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
            Self::ToolSelect => "Switch to the select tool",
            Self::ToolCrop => "Switch to the crop tool",
            Self::ToolDraw => "Switch to the draw tool",
            Self::ToolText => "Switch to the text tool",
            Self::BrushSizeUp => "Increase the draw tool brush size",
            Self::BrushSizeDown => "Decrease the draw tool brush size",
            Self::TogglePlayback => "Pause or resume animation playback",
            Self::FrameFirst => "Jump to the first frame",
            Self::FrameLast => "Jump to the last frame",
            Self::FrameNext => "Step forward one frame",
            Self::FramePrev => "Step back one frame",
            Self::ToggleMute => "Mute or unmute audio",
            Self::ToggleInfoPanel => "Show or hide the image info panel",
            Self::ToggleEditPanel => "Show or hide the edit panel",
            Self::ToggleCheckerboard => "Show or hide the checkerboard background",
            Self::ToggleBottomBar => "Show or hide the bottom toolbar",
            Self::OpenMedia => "Open a media file from disk",
            Self::CopyImage => "Copy the current image to the clipboard",
            Self::ExportImage => "Export the current image to a file",
        }
    }

    pub fn category(&self) -> KeyCategory {
        match self {
            Self::Next
            | Self::Previous
            | Self::ToggleFullscreen
            | Self::PasteFromClipboard
            | Self::OpenMedia
            | Self::CopyImage
            | Self::ExportImage => KeyCategory::Navigation,
            Self::RotateCw
            | Self::RotateCcw
            | Self::ZoomIn
            | Self::ZoomOut
            | Self::ZoomFit
            | Self::ZoomPreset(_)
            | Self::FocusScale
            | Self::UiScaleUp
            | Self::UiScaleDown
            | Self::UiScaleReset
            | Self::ToggleInfoPanel
            | Self::ToggleEditPanel
            | Self::ToggleCheckerboard
            | Self::ToggleBottomBar => KeyCategory::View,
            Self::ToolSelect
            | Self::ToolCrop
            | Self::ToolDraw
            | Self::ToolText
            | Self::BrushSizeUp
            | Self::BrushSizeDown => KeyCategory::Tools,
            Self::TogglePlayback
            | Self::FrameFirst
            | Self::FrameLast
            | Self::FrameNext
            | Self::FramePrev
            | Self::ToggleMute => KeyCategory::Playback,
        }
    }

    pub fn is_visible(&self) -> bool {
        Self::all_visible().contains(self)
    }

    pub fn all_visible() -> &'static [Action] {
        &[
            Action::Next,
            Action::Previous,
            Action::ToggleFullscreen,
            Action::OpenMedia,
            Action::CopyImage,
            Action::PasteFromClipboard,
            Action::ExportImage,
            Action::RotateCw,
            Action::RotateCcw,
            Action::ZoomIn,
            Action::ZoomOut,
            Action::ZoomFit,
            Action::FocusScale,
            Action::UiScaleUp,
            Action::UiScaleDown,
            Action::UiScaleReset,
            Action::ToggleInfoPanel,
            Action::ToggleEditPanel,
            Action::ToggleCheckerboard,
            Action::ToggleBottomBar,
            Action::ToolSelect,
            Action::ToolCrop,
            Action::ToolDraw,
            Action::ToolText,
            Action::BrushSizeUp,
            Action::BrushSizeDown,
            Action::TogglePlayback,
            Action::FrameFirst,
            Action::FramePrev,
            Action::FrameNext,
            Action::FrameLast,
            Action::ToggleMute,
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

    fn display_with(&self, key: fn(key::Code) -> &'static str) -> String {
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
        s.push_str(key(self.code));
        s
    }

    pub fn display(&self) -> String {
        self.display_with(code_name)
    }

    pub fn display_pretty(&self) -> String {
        self.display_with(code_display)
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

fn code_display(code: key::Code) -> &'static str {
    match code {
        key::Code::ArrowRight => "Right",
        key::Code::ArrowLeft => "Left",
        key::Code::ArrowUp => "Up",
        key::Code::ArrowDown => "Down",
        key::Code::Equal => "=",
        key::Code::Minus => "-",
        key::Code::BracketLeft => "[",
        key::Code::BracketRight => "]",
        key::Code::Backslash => "\\",
        key::Code::Semicolon => ";",
        key::Code::Quote => "'",
        key::Code::Comma => ",",
        key::Code::Period => ".",
        key::Code::Slash => "/",
        key::Code::Backquote => "`",
        key::Code::Digit0 => "0",
        key::Code::Digit1 => "1",
        key::Code::Digit2 => "2",
        key::Code::Digit3 => "3",
        key::Code::Digit4 => "4",
        key::Code::Digit5 => "5",
        key::Code::Digit6 => "6",
        key::Code::Digit7 => "7",
        key::Code::Digit8 => "8",
        key::Code::Digit9 => "9",
        _ => code_name(code),
    }
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
        m.insert(Action::RotateCw, n(key::Code::KeyR));
        m.insert(
            Action::RotateCcw,
            KeyBinding {
                ctrl: false,
                shift: true,
                alt: false,
                code: key::Code::KeyR,
            },
        );
        m.insert(Action::ToolSelect, n(key::Code::KeyS));
        m.insert(Action::ToolCrop, n(key::Code::KeyC));
        m.insert(Action::ToolDraw, n(key::Code::KeyD));
        m.insert(Action::ToolText, n(key::Code::KeyT));
        m.insert(Action::BrushSizeUp, n(key::Code::BracketRight));
        m.insert(Action::BrushSizeDown, n(key::Code::BracketLeft));
        m.insert(Action::TogglePlayback, n(key::Code::Space));
        m.insert(Action::FrameFirst, n(key::Code::Home));
        m.insert(Action::FrameLast, n(key::Code::End));
        m.insert(Action::FrameNext, n(key::Code::Period));
        m.insert(Action::FramePrev, n(key::Code::Comma));
        m.insert(Action::ToggleMute, n(key::Code::KeyM));
        m.insert(Action::ToggleInfoPanel, n(key::Code::KeyI));
        m.insert(Action::ToggleEditPanel, n(key::Code::KeyE));
        m.insert(Action::ToggleCheckerboard, n(key::Code::KeyB));
        m.insert(Action::ToggleBottomBar, n(key::Code::KeyH));
        m.insert(Action::OpenMedia, c(key::Code::KeyO));
        m.insert(Action::CopyImage, c(key::Code::KeyC));
        m.insert(Action::ExportImage, c(key::Code::KeyE));
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

    pub fn set(&mut self, action: Action, binding: KeyBinding) -> Vec<Action> {
        let mut displaced = Vec::new();
        self.bindings.retain(|a, b| {
            if *b == binding && *a != action {
                displaced.push(*a);
                false
            } else {
                true
            }
        });
        self.bindings.insert(action, binding);
        displaced
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
    pub tool_select: Option<String>,
    pub tool_crop: Option<String>,
    pub tool_draw: Option<String>,
    pub tool_text: Option<String>,
    pub brush_size_up: Option<String>,
    pub brush_size_down: Option<String>,
    pub toggle_playback: Option<String>,
    pub frame_first: Option<String>,
    pub frame_last: Option<String>,
    pub frame_next: Option<String>,
    pub frame_prev: Option<String>,
    pub toggle_mute: Option<String>,
    pub toggle_info_panel: Option<String>,
    pub toggle_edit_panel: Option<String>,
    pub toggle_checkerboard: Option<String>,
    pub toggle_bottom_bar: Option<String>,
    pub open_media: Option<String>,
    pub copy_image: Option<String>,
    pub export_image: Option<String>,
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
            tool_select: bind(Action::ToolSelect),
            tool_crop: bind(Action::ToolCrop),
            tool_draw: bind(Action::ToolDraw),
            tool_text: bind(Action::ToolText),
            brush_size_up: bind(Action::BrushSizeUp),
            brush_size_down: bind(Action::BrushSizeDown),
            toggle_playback: bind(Action::TogglePlayback),
            frame_first: bind(Action::FrameFirst),
            frame_last: bind(Action::FrameLast),
            frame_next: bind(Action::FrameNext),
            frame_prev: bind(Action::FramePrev),
            toggle_mute: bind(Action::ToggleMute),
            toggle_info_panel: bind(Action::ToggleInfoPanel),
            toggle_edit_panel: bind(Action::ToggleEditPanel),
            toggle_checkerboard: bind(Action::ToggleCheckerboard),
            toggle_bottom_bar: bind(Action::ToggleBottomBar),
            open_media: bind(Action::OpenMedia),
            copy_image: bind(Action::CopyImage),
            export_image: bind(Action::ExportImage),
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
            resolve(f.tool_select, Action::ToolSelect),
            resolve(f.tool_crop, Action::ToolCrop),
            resolve(f.tool_draw, Action::ToolDraw),
            resolve(f.tool_text, Action::ToolText),
            resolve(f.brush_size_up, Action::BrushSizeUp),
            resolve(f.brush_size_down, Action::BrushSizeDown),
            resolve(f.toggle_playback, Action::TogglePlayback),
            resolve(f.frame_first, Action::FrameFirst),
            resolve(f.frame_last, Action::FrameLast),
            resolve(f.frame_next, Action::FrameNext),
            resolve(f.frame_prev, Action::FramePrev),
            resolve(f.toggle_mute, Action::ToggleMute),
            resolve(f.toggle_info_panel, Action::ToggleInfoPanel),
            resolve(f.toggle_edit_panel, Action::ToggleEditPanel),
            resolve(f.toggle_checkerboard, Action::ToggleCheckerboard),
            resolve(f.toggle_bottom_bar, Action::ToggleBottomBar),
            resolve(f.open_media, Action::OpenMedia),
            resolve(f.copy_image, Action::CopyImage),
            resolve(f.export_image, Action::ExportImage),
        ]
        .into_iter()
        .flatten()
        .collect();
        Self { bindings }
    }
}
