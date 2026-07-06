use std::sync::OnceLock;

use iced::alignment::{Horizontal, Vertical};
use iced::font::Weight;
use iced::keyboard::{
    self,
    key::{self, Physical},
};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::tooltip::Position;
use iced::widget::{
    Space, button, column, container, image, pick_list, row, rule, scrollable, text, toggler,
};
use iced::{Color, Element, Font, Length, Theme};

use crate::app::Message;
use crate::config::{Config, PIXEL_PREVIEW_SIZE_OPTIONS, UI_SCALE_MAX, UI_SCALE_MIN};
use crate::keybinds::{Action, KeyBinding, KeyCategory, Keymap};
use crate::styles::{
    BAR_HEIGHT, BUTTON_SIZE, PAD, PREF_CONTENT_MAX_WIDTH, PREF_SIDEBAR_WIDTH, RULE_HEIGHT,
    bar_style, capturing_chip_style, key_chip_style, muted_text, panel_divider_style,
    plain_icon_button_style, pref_nav_button_style, pref_section_rule_style, set_radius,
};
use crate::ui::{svg_button_plain, with_tooltip};
use crate::widgets::hover_row::HoverRow;
use crate::widgets::logo_bloom::LogoBloom;
use crate::widgets::scale_entry::ScaleEntry;
use crate::widgets::theme_picker::ThemePicker;

fn on_wayland() -> bool {
    static ON_WAYLAND: OnceLock<bool> = OnceLock::new();
    *ON_WAYLAND.get_or_init(|| std::env::var_os("WAYLAND_DISPLAY").is_some())
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefSection {
    #[default]
    Appearance,
    Rendering,
    Keybindings,
    About,
}

#[derive(Debug, Clone)]
pub struct KeybindConflict {
    pub winner: Action,
    pub losers: Vec<Action>,
    pub binding: KeyBinding,
}

#[derive(Default)]
pub struct PreferenceState {
    pub capturing: Option<Action>,
    pub section: PrefSection,
    pub conflict: Option<KeybindConflict>,
}

#[derive(Debug, Clone)]
pub enum PreferenceMessage {
    SelectSection(PrefSection),
    SetTheme(Theme),
    SetRounded(bool),
    SetDecorations(bool),
    SetAlwaysOnTop(bool),
    SetUiScale(f32),
    SetAutoplay(bool),
    SetLoopAnimations(bool),
    SetLoopVideo(bool),
    SetRememberLast(bool),
    SetMipmapZoomOut(bool),
    SetSmoothZoomIn(bool),
    SetPixelGrid(bool),
    SetPixelPreviewSize(u32),
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

pub fn capture_key(
    state: &PreferenceState,
    physical_key: Physical,
    modifiers: keyboard::Modifiers,
) -> Option<PreferenceMessage> {
    let action = state.capturing?;
    let Physical::Code(code) = physical_key else {
        return None;
    };
    let is_modifier = matches!(
        code,
        key::Code::ControlLeft
            | key::Code::ControlRight
            | key::Code::ShiftLeft
            | key::Code::ShiftRight
            | key::Code::AltLeft
            | key::Code::AltRight
            | key::Code::SuperLeft
            | key::Code::SuperRight
    );
    if is_modifier {
        return None;
    }
    if code == key::Code::Escape {
        return Some(PreferenceMessage::CancelCapture);
    }
    if code == key::Code::Backspace
        && !(modifiers.control() || modifiers.shift() || modifiers.alt())
    {
        return Some(PreferenceMessage::ClearKeybinding(action));
    }
    Some(PreferenceMessage::SetKeybinding(
        action,
        KeyBinding {
            ctrl: modifiers.control(),
            shift: modifiers.shift(),
            alt: modifiers.alt(),
            code,
        },
    ))
}

pub fn update(
    msg: PreferenceMessage,
    pending: &mut Config,
    preference_state: &mut PreferenceState,
) -> PreferenceOutcome {
    preference_state.conflict = None;
    match msg {
        PreferenceMessage::SelectSection(s) => {
            preference_state.section = s;
            preference_state.capturing = None;
            PreferenceOutcome::Open
        }
        PreferenceMessage::SetTheme(t) => {
            pending.theme = t;
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
        PreferenceMessage::SetUiScale(v) => {
            pending.ui_scale = v.clamp(UI_SCALE_MIN, UI_SCALE_MAX);
            PreferenceOutcome::Open
        }
        PreferenceMessage::SetAutoplay(v) => {
            pending.autoplay = v;
            PreferenceOutcome::Open
        }
        PreferenceMessage::SetLoopAnimations(v) => {
            pending.loop_animations = v;
            PreferenceOutcome::Open
        }
        PreferenceMessage::SetLoopVideo(v) => {
            pending.loop_video = v;
            PreferenceOutcome::Open
        }
        PreferenceMessage::SetRememberLast(v) => {
            pending.remember_last = v;
            PreferenceOutcome::Open
        }
        PreferenceMessage::SetMipmapZoomOut(v) => {
            pending.mipmap_zoom_out = v;
            PreferenceOutcome::Open
        }
        PreferenceMessage::SetSmoothZoomIn(v) => {
            pending.smooth_zoom_in = v;
            PreferenceOutcome::Open
        }
        PreferenceMessage::SetPixelGrid(v) => {
            pending.show_pixel_grid = v;
            PreferenceOutcome::Open
        }
        PreferenceMessage::SetPixelPreviewSize(v) => {
            pending.pixel_preview_size = v;
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
            let losers = pending.keymap.set(action, kb);
            if !losers.is_empty() {
                preference_state.conflict = Some(KeybindConflict {
                    winner: action,
                    losers,
                    binding: kb,
                });
            }
            preference_state.capturing = None;
            PreferenceOutcome::Open
        }
        PreferenceMessage::ClearKeybinding(action) => {
            pending.keymap.remove(&action);
            preference_state.capturing = None;
            PreferenceOutcome::Open
        }
        PreferenceMessage::ResetAppearance => {
            let d = Config::default();
            pending.theme = d.theme;
            pending.rounded = d.rounded;
            pending.decorations = d.decorations;
            pending.always_on_top = d.always_on_top;
            pending.ui_scale = d.ui_scale;
            PreferenceOutcome::Open
        }
        PreferenceMessage::ResetRendering => {
            let d = Config::default();
            pending.autoplay = d.autoplay;
            pending.loop_animations = d.loop_animations;
            pending.loop_video = d.loop_video;
            pending.remember_last = d.remember_last;
            pending.mipmap_zoom_out = d.mipmap_zoom_out;
            pending.smooth_zoom_in = d.smooth_zoom_in;
            pending.show_pixel_grid = d.show_pixel_grid;
            pending.pixel_preview_size = d.pixel_preview_size;
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
            set_radius(pending.rounded);
            PreferenceOutcome::Save
        }
        PreferenceMessage::Cancel => {
            preference_state.capturing = None;
            PreferenceOutcome::Cancel
        }
    }
}

fn label_block<'a>(
    title: impl text::IntoFragment<'a>,
    description: impl text::IntoFragment<'a>,
    note: Option<(String, Color)>,
    theme: &Theme,
) -> Element<'a, Message> {
    let muted = muted_text(theme);
    let mut col = column![
        text(title).size(13),
        text(description).size(11).color(muted),
    ]
    .spacing(PAD / 2.0);
    if let Some((note, color)) = note {
        col = col.push(text(note).size(11).color(color));
    }
    container(col).clip(true).width(Length::Fill).into()
}

fn setting<'a>(
    label: &'a str,
    description: &'a str,
    control: Element<'a, Message>,
    theme: &Theme,
) -> Element<'a, Message> {
    HoverRow::new(
        row![label_block(label, description, None, theme), control]
            .align_y(Vertical::Center)
            .spacing(PAD * 2.0),
    )
    .into()
}

const CLEAR_SLOT: f32 = BUTTON_SIZE + PAD * 2.0;

fn keybind_row<'a>(
    action: Action,
    keymap: &Keymap,
    state: &PreferenceState,
    theme: &Theme,
) -> Element<'a, Message> {
    let is_capturing = state.capturing == Some(action);
    let binding = keymap.binding_for(&action);

    let chip: Element<'a, Message> = if is_capturing {
        button(text("Press a key…").size(11))
            .style(capturing_chip_style)
            .on_press(Message::Preference(PreferenceMessage::CancelCapture))
            .padding([4.0, 8.0])
            .into()
    } else {
        let label = binding
            .map(|kb| kb.display_pretty())
            .unwrap_or_else(|| "—".into());
        with_tooltip(
            button(text(label).size(11))
                .style(key_chip_style)
                .on_press(Message::Preference(PreferenceMessage::StartCapture(action)))
                .padding([4.0, 8.0]),
            "Edit keybinding",
            Position::Top,
        )
    };

    let note = if is_capturing {
        let hint = if binding.is_some() {
            "Backspace removes the binding, Esc cancels"
        } else {
            "Esc cancels"
        };
        Some((hint.into(), muted_text(theme)))
    } else {
        state
            .conflict
            .as_ref()
            .filter(|c| c.winner == action)
            .map(|c| {
                let losers = c
                    .losers
                    .iter()
                    .map(|l| l.label_with_detail())
                    .collect::<Vec<_>>()
                    .join(", ");
                let restore = if c.losers.iter().any(|l| !l.is_visible()) {
                    " (restore with Reset)"
                } else {
                    ""
                };
                (
                    format!("{} was unbound from {losers}{restore}", c.binding.display()),
                    theme.extended_palette().warning.base.color,
                )
            })
    };

    let clear: Option<Element<'a, Message>> = (binding.is_some() && !is_capturing).then(|| {
        with_tooltip(
            svg_button_plain(
                include_bytes!("../../assets/icons/close.svg"),
                Message::Preference(PreferenceMessage::ClearKeybinding(action)),
            ),
            "Remove keybinding",
            Position::Top,
        )
    });

    HoverRow::new(label_block(
        action.label_with_detail(),
        action.description(),
        note,
        theme,
    ))
    .trailing(chip)
    .hover_slot(CLEAR_SLOT, clear)
    .into()
}

fn subgroup<'a>(
    label: &'a str,
    rows: Vec<Element<'a, Message>>,
    theme: &Theme,
) -> Element<'a, Message> {
    subgroup_with_reset(label, None, rows, theme)
}

fn subgroup_with_reset<'a>(
    label: &'a str,
    reset: Option<(&'a str, PreferenceMessage)>,
    rows: Vec<Element<'a, Message>>,
    theme: &Theme,
) -> Element<'a, Message> {
    let header_color = theme.extended_palette().background.base.text;
    let label_text = text(label)
        .size(14)
        .font(Font {
            weight: Weight::Semibold,
            ..Font::DEFAULT
        })
        .color(header_color);

    let header: Element<'a, Message> = match reset {
        Some((tooltip, on_reset)) => row![
            label_text,
            Space::new().width(Length::Fill),
            with_tooltip(
                button(text("Reset").size(11))
                    .style(plain_icon_button_style)
                    .on_press(Message::Preference(on_reset))
                    .padding([2.0, 6.0]),
                tooltip,
                Position::Top,
            ),
        ]
        .align_y(Vertical::Center)
        .into(),
        None => label_text.into(),
    };

    column![
        column![header, rule::horizontal(1).style(pref_section_rule_style)].spacing(PAD),
        settings_list(rows),
    ]
    .spacing(PAD * 2.0)
    .width(Length::Fill)
    .into()
}

fn nav_button<'a>(label: &'a str, target: PrefSection, active: bool) -> Element<'a, Message> {
    button(text(label).size(13))
        .width(Length::Fill)
        .padding([6.0, 8.0])
        .style(pref_nav_button_style(active))
        .on_press(Message::Preference(PreferenceMessage::SelectSection(
            target,
        )))
        .into()
}

fn divider<'a>() -> Element<'a, Message> {
    container(Space::new().height(RULE_HEIGHT))
        .width(Length::Fill)
        .style(panel_divider_style)
        .into()
}

fn bar<'a>(content: impl Into<Element<'a, Message>>, divider_on_top: bool) -> Element<'a, Message> {
    let body = container(content)
        .width(Length::Fill)
        .height(Length::Fixed(BAR_HEIGHT))
        .align_x(Horizontal::Center)
        .align_y(Vertical::Center)
        .padding([0.0, PAD]);
    let stack = if divider_on_top {
        column![divider(), body]
    } else {
        column![body, divider()]
    }
    .width(Length::Fill);
    container(stack).width(Length::Fill).style(bar_style).into()
}

fn settings_list<'a>(rows: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    let mut col = column![].spacing(PAD * 3.0).width(Length::Fill);
    for row in rows {
        col = col.push(row);
    }
    col.into()
}

fn appearance_pane<'a>(pending: &'a Config, theme: &Theme) -> Element<'a, Message> {
    let rows = vec![
        setting(
            "Theme",
            "Color scheme for the application",
            ThemePicker::new(pending.theme.clone(), |t| {
                Message::Preference(PreferenceMessage::SetTheme(t))
            })
            .into(),
            theme,
        ),
        setting(
            "Rounded corners",
            "Use rounded corners on UI elements",
            toggler(pending.rounded)
                .on_toggle(|v| Message::Preference(PreferenceMessage::SetRounded(v)))
                .into(),
            theme,
        ),
        setting(
            "Window decorations",
            "Show the native title bar and window border",
            toggler(pending.decorations)
                .on_toggle(|v| Message::Preference(PreferenceMessage::SetDecorations(v)))
                .into(),
            theme,
        ),
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
        setting(
            "UI scale",
            "Scale the application interface",
            ScaleEntry::new(pending.ui_scale, |v| {
                Message::Preference(PreferenceMessage::SetUiScale(v))
            })
            .into(),
            theme,
        ),
    ];
    subgroup_with_reset(
        "Appearance",
        Some((
            "Reset appearance to defaults",
            PreferenceMessage::ResetAppearance,
        )),
        rows,
        theme,
    )
}

fn rendering_pane<'a>(pending: &'a Config, theme: &Theme) -> Element<'a, Message> {
    let playback = vec![
        setting(
            "Autoplay animations",
            "Automatically play animations when opened",
            toggler(pending.autoplay)
                .on_toggle(|v| Message::Preference(PreferenceMessage::SetAutoplay(v)))
                .into(),
            theme,
        ),
        setting(
            "Loop animations",
            "Restart GIF, APNG, and WebP animations automatically when they reach the end",
            toggler(pending.loop_animations)
                .on_toggle(|v| Message::Preference(PreferenceMessage::SetLoopAnimations(v)))
                .into(),
            theme,
        ),
        setting(
            "Loop video",
            if cfg!(feature = "av") {
                "Restart videos automatically when they reach the end"
            } else {
                "Video support is not built in this version"
            },
            {
                let t = toggler(pending.loop_video);
                if cfg!(feature = "av") {
                    t.on_toggle(|v| Message::Preference(PreferenceMessage::SetLoopVideo(v)))
                } else {
                    t
                }
            }
            .into(),
            theme,
        ),
    ];

    let files = vec![setting(
        "Remember last media",
        "Open the last viewed file when no file is passed on launch",
        toggler(pending.remember_last)
            .on_toggle(|v| Message::Preference(PreferenceMessage::SetRememberLast(v)))
            .into(),
        theme,
    )];

    let quality = vec![
        setting(
            "Zoom out filtering",
            "Trilinear mipmapping, pre-averages the image at smaller sizes to reduce aliasing (uses ~33% more VRAM, restart required)",
            toggler(pending.mipmap_zoom_out)
                .on_toggle(|v| Message::Preference(PreferenceMessage::SetMipmapZoomOut(v)))
                .into(),
            theme,
        ),
        setting(
            "Zoom in filtering",
            "Bilinear filtering, blends between neighbouring pixels when zoomed above 100%",
            toggler(pending.smooth_zoom_in)
                .on_toggle(|v| Message::Preference(PreferenceMessage::SetSmoothZoomIn(v)))
                .into(),
            theme,
        ),
        setting(
            "Pixel grid",
            "Overlay a grid aligned to pixel boundaries when zoomed in far",
            toggler(pending.show_pixel_grid)
                .on_toggle(|v| Message::Preference(PreferenceMessage::SetPixelGrid(v)))
                .into(),
            theme,
        ),
        setting(
            "Pixel preview size",
            "Grid size of the zoomed pixel preview in the info panel",
            pick_list(
                PIXEL_PREVIEW_SIZE_OPTIONS,
                Some(pending.pixel_preview_size),
                |v| Message::Preference(PreferenceMessage::SetPixelPreviewSize(v)),
            )
            .text_size(12)
            .into(),
            theme,
        ),
    ];

    column![
        subgroup_with_reset(
            "Playback",
            Some((
                "Reset these settings to defaults",
                PreferenceMessage::ResetRendering
            )),
            playback,
            theme
        ),
        subgroup("Files", files, theme),
        subgroup("Image Quality", quality, theme),
    ]
    .spacing(PAD * 5.0)
    .width(Length::Fill)
    .into()
}

fn keybindings_pane<'a>(
    pending: &'a Config,
    state: &PreferenceState,
    theme: &Theme,
) -> Element<'a, Message> {
    let mut col = column![].spacing(PAD * 5.0).width(Length::Fill);

    let mut first = true;
    for &category in KeyCategory::all() {
        let rows: Vec<Element<'a, Message>> = Action::all_visible()
            .iter()
            .filter(|a| a.category() == category)
            .map(|&action| keybind_row(action, &pending.keymap, state, theme))
            .collect();
        if rows.is_empty() {
            continue;
        }
        let reset = first.then_some((
            "Reset keybindings to defaults",
            PreferenceMessage::ResetKeybindings,
        ));
        col = col.push(subgroup_with_reset(category.label(), reset, rows, theme));
        first = false;
    }
    col.into()
}

fn logo_handle() -> image::Handle {
    static LOGO: OnceLock<image::Handle> = OnceLock::new();
    LOGO.get_or_init(|| {
        image::Handle::from_bytes(include_bytes!("../../assets/logo/bloom64.png").as_slice())
    })
    .clone()
}

fn about_pane<'a>(theme: &Theme) -> Element<'a, Message> {
    let muted = muted_text(theme);

    let link = |label: &'a str, url: &'static str| {
        with_tooltip(
            button(text(label).size(12))
                .style(plain_icon_button_style)
                .on_press(Message::OpenUrl(url))
                .padding([4.0, 8.0]),
            url,
            Position::Bottom,
        )
    };

    column![
        row![
            LogoBloom::new(logo_handle(), 64.0),
            column![
                text("Bloom").size(24).font(Font {
                    weight: Weight::Semibold,
                    ..Font::DEFAULT
                }),
                text(concat!("Version ", env!("CARGO_PKG_VERSION")))
                    .size(12)
                    .color(muted),
            ]
            .spacing(PAD / 2.0),
        ]
        .spacing(PAD * 2.0)
        .align_y(Vertical::Center),
        Space::new().height(PAD),
        text(env!("CARGO_PKG_DESCRIPTION")).size(13),
        Space::new().height(PAD),
        row![
            link("GitHub", env!("CARGO_PKG_REPOSITORY")),
            link(
                "Report an issue",
                concat!(env!("CARGO_PKG_REPOSITORY"), "/issues")
            ),
        ]
        .spacing(PAD),
        Space::new().height(PAD * 2.0),
        text(concat!("Licensed under ", env!("CARGO_PKG_LICENSE")))
            .size(11)
            .color(muted),
    ]
    .spacing(PAD)
    .align_x(Horizontal::Center)
    .width(Length::Fill)
    .into()
}

pub fn view<'a>(
    pending: &'a Config,
    theme: &Theme,
    preference_state: &'a PreferenceState,
) -> Element<'a, Message> {
    let header = bar(text("Preferences").size(16), false);

    let active = preference_state.section;
    let sidebar = container(
        column![
            nav_button(
                "Appearance",
                PrefSection::Appearance,
                active == PrefSection::Appearance
            ),
            nav_button(
                "Playback & Quality",
                PrefSection::Rendering,
                active == PrefSection::Rendering
            ),
            nav_button(
                "Keybindings",
                PrefSection::Keybindings,
                active == PrefSection::Keybindings
            ),
            Space::new().height(Length::Fill),
            nav_button("About", PrefSection::About, active == PrefSection::About),
        ]
        .spacing(PAD)
        .width(Length::Fill)
        .height(Length::Fill),
    )
    .width(Length::Fixed(PREF_SIDEBAR_WIDTH))
    .height(Length::Fill)
    .padding(PAD * 2.0);

    let pane = match active {
        PrefSection::Appearance => appearance_pane(pending, theme),
        PrefSection::Rendering => rendering_pane(pending, theme),
        PrefSection::Keybindings => keybindings_pane(pending, preference_state, theme),
        PrefSection::About => about_pane(theme),
    };

    let content: Element<'a, Message> = if active == PrefSection::About {
        container(pane)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Horizontal::Center)
            .align_y(Vertical::Center)
            .padding(PAD * 3.0)
            .into()
    } else {
        scrollable(
            container(pane)
                .max_width(PREF_CONTENT_MAX_WIDTH)
                .width(Length::Fill)
                .padding(PAD * 3.0),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .direction(Direction::Vertical(
            Scrollbar::new().width(4).scroller_width(4),
        ))
        .into()
    };

    let footer = bar(
        row![
            with_tooltip(
                button(text("Reset all").size(12))
                    .style(plain_icon_button_style)
                    .on_press(Message::Preference(PreferenceMessage::ResetAll))
                    .padding([4.0, 8.0]),
                "Reset all settings to defaults",
                Position::Top,
            ),
            Space::new().width(Length::Fill),
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
        .width(Length::Fill)
        .align_y(Vertical::Center)
        .spacing(PAD),
        true,
    );

    column![
        header,
        row![sidebar, rule::vertical(1), content]
            .width(Length::Fill)
            .height(Length::Fill),
        footer,
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}
