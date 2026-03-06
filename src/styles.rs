use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use iced::{
    Background, Color, Theme,
    widget::{button, container, svg},
};

pub const PAD: f32 = 5.0;
pub const TOOLTIP_DELAY: Duration = Duration::from_millis(400);
pub const BUTTON_SIZE: f32 = 20.0;
pub const BAR_HEIGHT: f32 = 40.0;
pub const RADIUS: f32 = 6.0;

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
