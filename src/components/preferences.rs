use std::sync::OnceLock;

use iced::alignment::{Horizontal, Vertical};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::tooltip::Position;
use iced::widget::{button, column, container, pick_list, row, rule, scrollable, text, toggler};
use iced::{Element, Length, Theme};

use crate::app::Message;
use crate::config::{ALL_THEMES, Config};
use crate::keybinds::{Action, KeyBinding, Keymap};
use crate::styles::{
    PAD, capturing_chip_style, key_chip_style, plain_icon_button_style, scrollbar_style, set_radius,
};
use crate::ui::{svg_button_plain, with_tooltip};
use crate::wgpu::view_program::ViewProgram;

fn on_wayland() -> bool {
    static ON_WAYLAND: OnceLock<bool> = OnceLock::new();
    *ON_WAYLAND.get_or_init(|| std::env::var_os("WAYLAND_DISPLAY").is_some())
}

#[derive(Default)]
pub struct PreferenceState {
    pub capturing: Option<Action>,
}

#[derive(Debug, Clone)]
pub enum PreferenceMessage {
    SetTheme(Theme),
    SetLanczos(bool),
    SetRounded(bool),
    SetDecorations(bool),
    SetAlwaysOnTop(bool),
    StartCapture(Action),
    CancelCapture,
    SetKeybinding(Action, KeyBinding),
    ClearKeybinding(Action),
    ResetAppearance,
    ResetRendering,
    ResetKeybindings,
    ResetAll,
    Save,
    Cancel,
}

pub enum PreferenceOutcome {
    Open,
    Save,
    Cancel,
}

pub fn update(
    msg: PreferenceMessage,
    pending: &mut Config,
    program: &mut ViewProgram,
    preference_state: &mut PreferenceState,
) -> PreferenceOutcome {
    match msg {
        PreferenceMessage::SetTheme(t) => {
            pending.theme = t;
            PreferenceOutcome::Open
        }
        PreferenceMessage::SetLanczos(v) => {
            pending.lanczos = v;
            PreferenceOutcome::Open
        }
        PreferenceMessage::SetRounded(v) => {
            pending.rounded = v;
            PreferenceOutcome::Open
        }
        PreferenceMessage::SetDecorations(v) => {
            pending.decorations = v;
            PreferenceOutcome::Open
        }
        PreferenceMessage::SetAlwaysOnTop(v) => {
            pending.always_on_top = v;
            PreferenceOutcome::Open
        }
        PreferenceMessage::StartCapture(action) => {
            preference_state.capturing = Some(action);
            PreferenceOutcome::Open
        }
        PreferenceMessage::CancelCapture => {
            preference_state.capturing = None;
            PreferenceOutcome::Open
        }
        PreferenceMessage::SetKeybinding(action, kb) => {
            pending.keymap.set(action, kb);
            preference_state.capturing = None;
            PreferenceOutcome::Open
        }
        PreferenceMessage::ClearKeybinding(action) => {
            pending.keymap.remove(&action);
            PreferenceOutcome::Open
        }
        PreferenceMessage::ResetAppearance => {
            let d = Config::default();
            pending.theme = d.theme;
            pending.rounded = d.rounded;
            pending.decorations = d.decorations;
            pending.always_on_top = d.always_on_top;
            PreferenceOutcome::Open
        }
        PreferenceMessage::ResetRendering => {
            pending.lanczos = Config::default().lanczos;
            PreferenceOutcome::Open
        }
        PreferenceMessage::ResetKeybindings => {
            pending.keymap = Keymap::default();
            preference_state.capturing = None;
            PreferenceOutcome::Open
        }
        PreferenceMessage::ResetAll => {
            *pending = Config::default();
            preference_state.capturing = None;
            PreferenceOutcome::Open
        }
        PreferenceMessage::Save => {
            program.lanczos_enabled = pending.lanczos;
            set_radius(pending.rounded);
            PreferenceOutcome::Save
        }
        PreferenceMessage::Cancel => {
            preference_state.capturing = None;
            PreferenceOutcome::Cancel
        }
    }
}

fn section<'a>(
    label: &'a str,
    tooltip: &'a str,
    on_reset: PreferenceMessage,
    theme: &Theme,
) -> Element<'a, Message> {
    let accent = theme.extended_palette().primary.base.color;
    column![
        row![
            text(label).size(11).color(accent),
            iced::widget::Space::new().width(Length::Fill),
            with_tooltip(
                button(text("Reset").size(11))
                    .style(plain_icon_button_style)
                    .on_press(Message::Preference(on_reset))
                    .padding([2.0, 6.0]),
                tooltip,
                Position::Top,
            ),
        ]
        .align_y(Vertical::Center),
        rule::horizontal(1),
    ]
    .spacing(PAD)
    .into()
}

fn setting<'a>(
    label: &'a str,
    description: &'a str,
    control: Element<'a, Message>,
    theme: &Theme,
) -> Element<'a, Message> {
    let muted = theme
        .extended_palette()
        .background
        .base
        .text
        .scale_alpha(0.5);
    row![
        column![
            text(label).size(13),
            text(description).size(11).color(muted),
        ]
        .spacing(PAD / 2.0)
        .width(Length::Fill),
        control,
    ]
    .align_y(Vertical::Center)
    .spacing(PAD * 2.0)
    .into()
}

fn keybind_row<'a>(
    action: Action,
    keymap: &Keymap,
    capturing: Option<Action>,
    theme: &Theme,
) -> Element<'a, Message> {
    let is_capturing = capturing == Some(action);
    let muted = theme
        .extended_palette()
        .background
        .base
        .text
        .scale_alpha(0.5);

    let chip: Element<'a, Message> = if is_capturing {
        button(text("Press a key…").size(11))
            .style(capturing_chip_style)
            .on_press(Message::Preference(PreferenceMessage::CancelCapture))
            .padding([4.0, 8.0])
            .into()
    } else {
        let label = keymap
            .binding_for(&action)
            .map(|kb| kb.display())
            .unwrap_or_else(|| "—".into());
        button(text(label).size(11))
            .style(key_chip_style)
            .on_press(Message::Preference(PreferenceMessage::StartCapture(action)))
            .padding([4.0, 8.0])
            .into()
    };

    let control: Element<'a, Message> = if keymap.binding_for(&action).is_some() && !is_capturing {
        row![
            chip,
            svg_button_plain(
                include_bytes!("../../assets/icons/close.svg"),
                Message::Preference(PreferenceMessage::ClearKeybinding(action)),
            ),
        ]
        .spacing(PAD)
        .align_y(Vertical::Center)
        .into()
    } else {
        chip
    };

    row![
        column![
            text(action.label_with_detail()).size(13),
            text(action.description()).size(11).color(muted),
        ]
        .spacing(PAD / 2.0)
        .width(Length::Fill),
        control,
    ]
    .align_y(Vertical::Center)
    .spacing(PAD * 2.0)
    .into()
}

pub fn view<'a>(
    pending: &'a Config,
    theme: &Theme,
    preference_state: &'a PreferenceState,
) -> Element<'a, Message> {
    let action_buttons = container(
        row![
            with_tooltip(
                button(text("Reset").size(12))
                    .style(plain_icon_button_style)
                    .on_press(Message::Preference(PreferenceMessage::ResetAll))
                    .padding([4.0, 8.0]),
                "Reset all settings to defaults",
                Position::Top,
            ),
            iced::widget::Space::new().width(Length::Fill),
            with_tooltip(
                svg_button_plain(
                    include_bytes!("../../assets/icons/check.svg"),
                    Message::Preference(PreferenceMessage::Save),
                ),
                "Save",
                Position::Top,
            ),
            with_tooltip(
                svg_button_plain(
                    include_bytes!("../../assets/icons/close.svg"),
                    Message::Preference(PreferenceMessage::Cancel),
                ),
                "Cancel",
                Position::Top,
            ),
        ]
        .align_y(Vertical::Center)
        .spacing(PAD),
    )
    .width(Length::Fill)
    .padding(PAD * 2.0);

    let keybind_rows: Vec<Element<'a, Message>> = Action::all_visible()
        .iter()
        .map(|&action| keybind_row(action, &pending.keymap, preference_state.capturing, theme))
        .collect();

    let content = column![
        container(text("Preferences").size(16))
            .width(Length::Fill)
            .align_x(Horizontal::Center),
        iced::widget::Space::new().height(PAD * 2.0),
        section(
            "Appearance",
            "Reset appearance to defaults",
            PreferenceMessage::ResetAppearance,
            theme
        ),
        iced::widget::Space::new().height(PAD),
        setting(
            "Theme",
            "Color scheme for the application",
            pick_list(ALL_THEMES, Some(&pending.theme), |t| {
                Message::Preference(PreferenceMessage::SetTheme(t))
            })
            .into(),
            theme,
        ),
        iced::widget::Space::new().height(PAD),
        setting(
            "Rounded corners",
            "Use rounded corners on UI elements",
            toggler(pending.rounded)
                .on_toggle(|v| Message::Preference(PreferenceMessage::SetRounded(v)))
                .into(),
            theme,
        ),
        iced::widget::Space::new().height(PAD),
        setting(
            "Window decorations",
            "Show the native title bar and window border",
            toggler(pending.decorations)
                .on_toggle(|v| Message::Preference(PreferenceMessage::SetDecorations(v)))
                .into(),
            theme,
        ),
        iced::widget::Space::new().height(PAD),
        setting(
            "Always on top",
            if on_wayland() {
                "Not supported on Wayland"
            } else {
                "Always show Bloom above other windows"
            },
            {
                let t = toggler(pending.always_on_top);
                if on_wayland() {
                    t
                } else {
                    t.on_toggle(|v| Message::Preference(PreferenceMessage::SetAlwaysOnTop(v)))
                }
            }
            .into(),
            theme,
        ),
        iced::widget::Space::new().height(PAD * 2.0),
        section(
            "Rendering",
            "Reset rendering to defaults",
            PreferenceMessage::ResetRendering,
            theme
        ),
        iced::widget::Space::new().height(PAD),
        setting(
            "Lanczos filtering",
            "High-quality downsampling when zoomed out. This is GPU intensive",
            toggler(pending.lanczos)
                .on_toggle(|v| Message::Preference(PreferenceMessage::SetLanczos(v)))
                .into(),
            theme,
        ),
        iced::widget::Space::new().height(PAD * 2.0),
        section(
            "Keybindings",
            "Reset keybindings to defaults",
            PreferenceMessage::ResetKeybindings,
            theme
        ),
        iced::widget::Space::new().height(PAD),
    ]
    .extend(keybind_rows)
    .spacing(PAD)
    .padding(PAD * 3.0)
    .width(Length::Fill);

    column![
        scrollable(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .direction(Direction::Vertical(
                Scrollbar::new().width(4).margin(4).scroller_width(4),
            ))
            .style(scrollbar_style),
        rule::horizontal(1),
        action_buttons,
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
