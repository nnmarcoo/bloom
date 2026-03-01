use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use iced::{
    Background, Border, Color, Theme,
    widget::{button, container, scrollable, svg},
};

pub const PAD: f32 = 5.0;
pub const TOOLTIP_DELAY: Duration = Duration::from_millis(400);
pub const BUTTON_SIZE: f32 = 20.0;
pub const BAR_HEIGHT: f32 = 40.0;
pub const RADIUS: f32 = 8.0;

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

pub fn darken(color: Color, factor: f32) -> Color {
    Color {
        r: (color.r * factor).clamp(0.0, 1.0),
        g: (color.g * factor).clamp(0.0, 1.0),
        b: (color.b * factor).clamp(0.0, 1.0),
        a: color.a,
    }
}

pub fn bar_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();
    let bar_bg = darken(palette.background.base.color, 0.85);

    container::Style {
        text_color: Some(palette.background.base.text),
        background: Some(Background::Color(bar_bg)),
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

pub fn scrollbar_style(theme: &Theme, _status: scrollable::Status) -> scrollable::Style {
    let palette = theme.extended_palette();
    let scroller_color = palette.background.strong.color;
    scrollable::Style {
        container: container::Style::default(),
        vertical_rail: scrollable::Rail {
            background: None,
            border: Border::default(),
            scroller: scrollable::Scroller {
                background: Background::Color(scroller_color),
                border: iced::border::rounded(radius()),
            },
        },
        horizontal_rail: scrollable::Rail {
            background: None,
            border: Border::default(),
            scroller: scrollable::Scroller {
                background: Background::Color(scroller_color),
                border: iced::border::rounded(radius()),
            },
        },
        gap: None,
        auto_scroll: scrollable::AutoScroll {
            background: Background::Color(palette.background.weak.color),
            border: Border::default(),
            shadow: iced::Shadow::default(),
            icon: palette.background.base.text,
        },
    }
}

pub fn menu_item_hover_color(theme: &Theme) -> Color {
    darken(theme.extended_palette().background.base.color, 1.15)
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
    let bar_bg = darken(palette.background.base.color, 0.85);

    let background = match status {
        button::Status::Hovered => Some(Background::Color(darken(bar_bg, 1.15))),
        button::Status::Pressed => Some(Background::Color(darken(bar_bg, 0.85))),
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
    let bg = palette.background.base.color;

    let background = match status {
        button::Status::Hovered => Some(Background::Color(darken(bg, 0.88))),
        button::Status::Pressed => Some(Background::Color(darken(bg, 0.78))),
        _ => None,
    };

    button::Style {
        background,
        border: iced::border::rounded(radius()),
        text_color: palette.background.base.text,
        ..Default::default()
    }
}

pub fn icon_button_active_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    let active_bg = palette.primary.weak.color;

    let background = match status {
        button::Status::Hovered => Some(Background::Color(darken(active_bg, 1.1))),
        button::Status::Pressed => Some(Background::Color(darken(active_bg, 0.85))),
        _ => Some(Background::Color(active_bg)),
    };

    button::Style {
        background,
        border: iced::border::rounded(radius()),
        text_color: palette.background.base.text,
        ..Default::default()
    }
}
