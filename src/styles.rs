use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use iced::{
    Background, Border, Color, Shadow, Theme, Vector,
    widget::{button, container, svg},
};

pub const PAD: f32 = 5.0;
pub const TOOLTIP_DELAY: Duration = Duration::from_millis(400);
pub const BUTTON_SIZE: f32 = 20.0;
pub const BAR_HEIGHT: f32 = 40.0;
pub const RADIUS: f32 = 6.0;
pub const INFO_PANEL_WIDTH: f32 = 220.0;
pub const RULE_HEIGHT: f32 = 2.0;
pub const EDIT_PANEL_WIDTH: f32 = 240.0;
pub const TOAST_WIDTH: f32 = 300.0;

static ACTIVE_RADIUS: OnceLock<AtomicU32> = OnceLock::new();

pub fn set_radius(rounded: bool) {
    let val = if rounded { RADIUS } else { 0.0 };
    ACTIVE_RADIUS
        .get_or_init(|| AtomicU32::new(val.to_bits()))
        .store(val.to_bits(), Ordering::Relaxed);
}

pub fn radius() -> f32 {
    f32::from_bits(
        ACTIVE_RADIUS
            .get_or_init(|| AtomicU32::new(RADIUS.to_bits()))
            .load(Ordering::Relaxed),
    )
}

pub fn bar_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        text_color: Some(palette.background.base.text),
        background: Some(Background::Color(palette.background.strong.color)),
        ..Default::default()
    }
}

pub fn svg_style(theme: &Theme, status: svg::Status) -> svg::Style {
    let palette = theme.extended_palette();
    let base = palette.background.base.text;

    let color = match status {
        svg::Status::Hovered => base,
        svg::Status::Idle => base.scale_alpha(0.7),
    };

    svg::Style { color: Some(color) }
}

pub fn spinner_bg_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        text_color: Some(palette.background.base.text),
        background: Some(Background::Color(
            palette.background.base.color.scale_alpha(0.9),
        )),
        border: iced::border::rounded(radius()),
        ..Default::default()
    }
}

pub fn menu_container_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        text_color: Some(palette.background.base.text),
        background: Some(Background::Color(palette.background.weak.color)),
        border: iced::Border {
            color: palette.background.strong.color,
            width: 1.0,
            radius: radius().into(),
        },
        ..Default::default()
    }
}

pub fn tooltip_style(theme: &Theme) -> container::Style {
    menu_container_style(theme)
}

pub fn menu_item_hover_color(theme: &Theme) -> Color {
    theme.extended_palette().background.strong.color
}

pub fn menu_separator_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(palette.background.strong.color)),
        ..Default::default()
    }
}

pub fn icon_button_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();

    let background = match status {
        button::Status::Hovered => Some(Background::Color(palette.background.base.color)),
        button::Status::Pressed => Some(Background::Color(palette.background.weak.color)),
        _ => None,
    };

    button::Style {
        background,
        border: iced::border::rounded(radius()),
        text_color: palette.background.base.text,
        ..Default::default()
    }
}

pub fn plain_icon_button_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();

    let background = match status {
        button::Status::Hovered => Some(Background::Color(palette.background.weak.color)),
        button::Status::Pressed => Some(Background::Color(palette.background.strong.color)),
        _ => None,
    };

    button::Style {
        background,
        border: iced::border::rounded(radius()),
        text_color: palette.background.base.text,
        ..Default::default()
    }
}

pub fn key_chip_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => {
            Some(Background::Color(palette.background.strong.color))
        }
        _ => Some(Background::Color(palette.background.weak.color)),
    };
    button::Style {
        background,
        border: iced::Border {
            color: palette.background.strong.color,
            width: 1.0,
            radius: radius().into(),
        },
        text_color: palette.background.base.text,
        ..Default::default()
    }
}

pub fn capturing_chip_style(theme: &Theme, _status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    button::Style {
        background: Some(Background::Color(
            palette.primary.weak.color.scale_alpha(0.3),
        )),
        border: iced::Border {
            color: palette.primary.base.color,
            width: 1.0,
            radius: radius().into(),
        },
        text_color: palette.background.base.text.scale_alpha(0.7),
        ..Default::default()
    }
}

pub fn info_section_header_style(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: None,
        border: iced::Border::default(),
        ..Default::default()
    }
}

pub fn icon_button_active_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();

    let background = match status {
        button::Status::Hovered | button::Status::Pressed => {
            Some(Background::Color(palette.primary.strong.color))
        }
        _ => Some(Background::Color(palette.primary.base.color)),
    };

    button::Style {
        background,
        border: iced::border::rounded(radius()),
        text_color: palette.background.base.text,
        ..Default::default()
    }
}

pub fn toast_container_style(theme: &Theme, accent: Color, alpha: f32) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        text_color: Some(palette.background.base.text.scale_alpha(alpha)),
        background: Some(Background::Color(
            palette.background.weak.color.scale_alpha(alpha),
        )),
        border: Border {
            color: accent.scale_alpha(alpha),
            width: 1.5,
            radius: radius().into(),
        },
        shadow: Shadow {
            color: Color::BLACK.scale_alpha(0.3 * alpha),
            offset: Vector::new(0.0, 2.0),
            blur_radius: 6.0,
        },
        snap: false,
    }
}

pub fn modifier_card_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(palette.background.base.color)),
        border: iced::border::rounded(radius()),
        ..Default::default()
    }
}

pub fn panel_divider_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    container::Style {
        background: Some(Background::Color(
            palette.background.base.text.scale_alpha(0.06),
        )),
        ..Default::default()
    }
}

pub fn toast_dismiss_style(theme: &Theme, status: button::Status, alpha: f32) -> button::Style {
    let palette = theme.extended_palette();
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => Some(Background::Color(
            palette.background.strong.color.scale_alpha(alpha),
        )),
        _ => None,
    };
    button::Style {
        background,
        border: iced::border::rounded(radius()),
        text_color: palette.background.base.text.scale_alpha(0.6 * alpha),
        ..Default::default()
    }
}
