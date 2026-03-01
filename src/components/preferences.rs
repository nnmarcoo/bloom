use iced::alignment::{Horizontal, Vertical};
use iced::widget::tooltip::Position;
use iced::widget::{column, container, pick_list, row, rule, scrollable, text, toggler};
use iced::{Element, Length, Theme};

use crate::app::Message;
use crate::config::Config;
use crate::styles::{PAD, set_radius};
use crate::ui::{svg_button_plain, with_tooltip};
use crate::wgpu::view_program::ViewProgram;

#[derive(Debug, Clone)]
pub enum PreferenceMessage {
    SetTheme(Theme),
    SetLanczos(bool),
    SetRounded(bool),
    Save,
    Cancel,
}

pub fn update(
    msg: PreferenceMessage,
    config: &mut Config,
    pending: &mut Config,
    program: &mut ViewProgram,
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
        PreferenceMessage::Save => {
            *config = pending.clone();
            program.lanczos_enabled = config.lanczos;
            set_radius(config.rounded);
            false
        }
        PreferenceMessage::Cancel => {
            *pending = config.clone();
            false
        }
    }
}

const ALL_THEMES: &[Theme] = &[
    Theme::Light,
    Theme::Dark,
    Theme::Dracula,
    Theme::Nord,
    Theme::SolarizedLight,
    Theme::SolarizedDark,
    Theme::GruvboxLight,
    Theme::GruvboxDark,
    Theme::CatppuccinLatte,
    Theme::CatppuccinFrappe,
    Theme::CatppuccinMacchiato,
    Theme::CatppuccinMocha,
    Theme::TokyoNight,
    Theme::TokyoNightStorm,
    Theme::TokyoNightLight,
    Theme::KanagawaWave,
    Theme::KanagawaDragon,
    Theme::KanagawaLotus,
    Theme::Moonfly,
    Theme::Nightfly,
    Theme::Oxocarbon,
    Theme::Ferra,
];

fn section<'a>(label: &'a str, theme: &Theme) -> Element<'a, Message> {
    let muted = theme
        .extended_palette()
        .background
        .base
        .text
        .scale_alpha(0.5);
    column![text(label).size(13).color(muted), rule::horizontal(1),]
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

pub fn view<'a>(pending: &'a Config, theme: &Theme) -> Element<'a, Message> {
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

    let theme_picker = pick_list(ALL_THEMES, Some(&pending.theme), |t| {
        Message::Preference(PreferenceMessage::SetTheme(t))
    });

    let rounded_toggle = toggler(pending.rounded)
        .on_toggle(|v| Message::Preference(PreferenceMessage::SetRounded(v)));

    let lanczos_toggle = toggler(pending.lanczos)
        .on_toggle(|v| Message::Preference(PreferenceMessage::SetLanczos(v)));

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
            theme_picker.into(),
            theme,
        ),
        iced::widget::Space::new().height(PAD),
        setting(
            "Rounded corners",
            "Use rounded corners on UI elements",
            rounded_toggle.into(),
            theme,
        ),
        iced::widget::Space::new().height(PAD * 2.0),
        section("Rendering", theme),
        iced::widget::Space::new().height(PAD),
        setting(
            "Lanczos filtering",
            "High-quality downsampling when zoomed out. This is GPU intensive",
            lanczos_toggle.into(),
            theme,
        ),
    ]
    .spacing(PAD)
    .padding(PAD * 3.0)
    .width(Length::Fill);

    iced::widget::stack![
        scrollable(content).width(Length::Fill).height(Length::Fill),
        action_buttons,
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
