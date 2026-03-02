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

#[derive(Default)]
pub struct PrefsState {
    pub capturing: Option<Action>,
}

#[derive(Debug, Clone)]
pub enum PreferenceMessage {
    SetTheme(Theme),
    SetLanczos(bool),
    SetRounded(bool),
    StartCapture(Action),
    CancelCapture,
    SetKeybinding(Action, KeyBinding),
    ClearKeybinding(Action),
    ResetKeybindings,
    Save,
    Cancel,
}

pub fn update(
    msg: PreferenceMessage,
    config: &mut Config,
    pending: &mut Config,
    program: &mut ViewProgram,
    prefs_state: &mut PrefsState,
) -> bool {
    match msg {
        PreferenceMessage::SetTheme(t) => {
            pending.theme = t;
            true
        }
        PreferenceMessage::SetLanczos(v) => {
            pending.lanczos = v;
            true
        }
        PreferenceMessage::SetRounded(v) => {
            pending.rounded = v;
            true
        }
        PreferenceMessage::StartCapture(action) => {
            prefs_state.capturing = Some(action);
            true
        }
        PreferenceMessage::CancelCapture => {
            prefs_state.capturing = None;
            true
        }
        PreferenceMessage::SetKeybinding(action, kb) => {
            pending.keymap.set(action, kb);
            prefs_state.capturing = None;
            true
        }
        PreferenceMessage::ClearKeybinding(action) => {
            pending.keymap.remove(&action);
            true
        }
        PreferenceMessage::ResetKeybindings => {
            pending.keymap.reset_to_defaults();
            true
        }
        PreferenceMessage::Save => {
            *config = pending.clone();
            program.lanczos_enabled = config.lanczos;
            set_radius(config.rounded);
            false
        }
        PreferenceMessage::Cancel => {
            *pending = config.clone();
            prefs_state.capturing = None;
            false
        }
    }
}

fn section<'a>(label: &'a str, theme: &Theme) -> Element<'a, Message> {
    let muted = theme
        .extended_palette()
        .background
        .base
        .text
        .scale_alpha(0.5);
    column![text(label).size(13).color(muted), rule::horizontal(1)]
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
    prefs_state: &'a PrefsState,
) -> Element<'a, Message> {
    let action_buttons = container(
        row![
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
        .spacing(PAD),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(Horizontal::Right)
    .align_y(Vertical::Bottom)
    .padding(PAD * 3.0);

    let reset_btn = container(
        button(text("Reset to defaults").size(12))
            .style(plain_icon_button_style)
            .on_press(Message::Preference(PreferenceMessage::ResetKeybindings))
            .padding([4.0, 8.0]),
    )
    .width(Length::Fill)
    .align_x(Horizontal::Right);

    let keybind_rows: Vec<Element<'a, Message>> = Action::all_visible()
        .iter()
        .map(|&action| keybind_row(action, &pending.keymap, prefs_state.capturing, theme))
        .collect();

    let content = column![
        container(text("Preferences").size(16))
            .width(Length::Fill)
            .align_x(Horizontal::Center),
        iced::widget::Space::new().height(PAD * 2.0),
        section("Appearance", theme),
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
        iced::widget::Space::new().height(PAD * 2.0),
        section("Rendering", theme),
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
        section("Keybindings", theme),
        iced::widget::Space::new().height(PAD),
        reset_btn,
        iced::widget::Space::new().height(PAD),
    ]
    .extend(keybind_rows)
    .spacing(PAD)
    .padding(PAD * 3.0)
    .width(Length::Fill);

    iced::widget::stack![
        scrollable(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .direction(Direction::Vertical(
                Scrollbar::new().width(4).margin(4).scroller_width(4),
            ))
            .style(scrollbar_style),
        action_buttons,
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
