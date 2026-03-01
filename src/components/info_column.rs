use std::path::Path;
use std::time::Duration;

use iced::alignment::{Horizontal, Vertical};
use iced::widget::tooltip::Position;
use iced::widget::{Space, column, container, row, scrollable, text, tooltip};
use iced::{Background, Element, Font, Length, border};

use crate::app::Message;
use crate::gallery::Gallery;
use crate::styles::{PAD, TOOLTIP_DELAY, bar_style, radius};
use crate::wgpu::view_program::ViewProgram;

fn row_item<'a>(lbl: &'a str, val: impl ToString) -> Element<'a, Message> {
    row![
        text(lbl)
            .size(12)
            .color([0.5, 0.5, 0.5])
            .font(Font::MONOSPACE)
            .width(Length::Fill),
        text(val.to_string())
            .size(12)
            .font(Font::MONOSPACE)
            .align_x(Horizontal::Right),
    ]
    .into()
}

fn color_row<'a>(rgba: [u8; 4]) -> Element<'a, Message> {
    let [r, g, b, a] = rgba;
    let color = iced::Color {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: a as f32 / 255.0,
    };
    let swatch = container(Space::new())
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_: &_| container::Style {
            background: Some(Background::Color(color)),
            border: border::rounded(radius()),
            ..Default::default()
        });
    let ch = |v: u8| {
        container(text(format!("{v}")).size(12).font(Font::MONOSPACE))
            .width(Length::Fixed(30.0))
            .align_x(Horizontal::Right)
    };
    row![
        text("RGBA")
            .size(12)
            .color([0.5, 0.5, 0.5])
            .font(Font::MONOSPACE),
        Space::new().width(10),
        swatch,
        ch(r),
        ch(g),
        ch(b),
        ch(a),
    ]
    .align_y(Vertical::Center)
    .into()
}

fn truncate_filename(name: &str, max_chars: usize) -> String {
    let (stem, ext) = match name.rfind('.') {
        Some(i) => (&name[..i], &name[i..]),
        None => (name, ""),
    };

    if name.chars().count() <= max_chars {
        return name.to_string();
    }

    let ext_chars = ext.chars().count();
    let reserved = 1 + ext_chars;
    if max_chars <= reserved {
        return name.chars().take(max_chars).collect();
    }

    let stem_budget = max_chars - reserved;
    let truncated_stem: String = stem.chars().take(stem_budget).collect();
    format!("{}~{}", truncated_stem, ext)
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

fn aspect_ratio_str(w: u32, h: u32) -> String {
    let d = gcd(w, h);
    format!("{}:{}", w / d, h / d)
}

fn format_duration(d: Duration) -> String {
    let ms = d.as_millis();
    let secs = ms / 1000;
    let rem = ms % 1000;
    if rem == 0 {
        format!("{secs}s")
    } else {
        format!("{secs}.{rem:03}s")
    }
}

pub fn view<'a>(
    path: Option<&'a Path>,
    gallery: &Gallery,
    program: &ViewProgram,
) -> Element<'a, Message> {
    if program.image_size().is_none() {
        return container(
            text("No image loaded")
                .size(12)
                .color([0.5, 0.5, 0.5])
                .font(Font::MONOSPACE),
        )
        .style(bar_style)
        .height(Length::Fill)
        .width(Length::Fixed(220.0))
        .align_x(Horizontal::Center)
        .align_y(Vertical::Center)
        .into();
    }

    let mut rows: Vec<Element<'a, Message>> = Vec::new();

    if let Some(p) = path {
        if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
            let filename_row = row_item("Filename", truncate_filename(name, 18));
            let entry = if let Some(path_str) = p.to_str() {
                tooltip(
                    filename_row,
                    container(text(path_str).size(11).font(Font::MONOSPACE))
                        .padding(PAD)
                        .style(container::rounded_box),
                    Position::Right,
                )
                .delay(TOOLTIP_DELAY)
                .into()
            } else {
                filename_row
            };
            rows.push(entry);
        }

        if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
            rows.push(row_item("Format", ext.to_ascii_uppercase()));
        }
    }

    let count = gallery.len();
    if count > 0 {
        rows.push(row_item(
            "In folder",
            format!("{} / {}", gallery.position() + 1, count),
        ));
    }

    if let Some((w, h)) = program.image_size() {
        rows.push(row_item("Dimensions", format!("{} x {}", w, h)));
        rows.push(row_item("Aspect ratio", aspect_ratio_str(w, h)));
    }

    rows.push(row_item(
        "Scale",
        format!("{:.0}%", program.scale() * 100.0),
    ));

    if let Some(size) = gallery.file_size() {
        rows.push(row_item("File size", format_size(size)));
    }

    if let Some(bytes) = program.decoded_size_bytes() {
        rows.push(row_item("RAM usage", format_size(bytes as u64)));
    }

    if let Some((frame, total)) = program.animation_info() {
        rows.push(row_item("Frame", format!("{} / {}", frame + 1, total)));
    }

    if let Some(dur) = program.animation_duration() {
        rows.push(row_item("Duration", format_duration(dur)));
    }

    if let Some((px, py, rgba)) = program.cursor_info() {
        rows.push(color_row(rgba));
        rows.push(row_item("Pixel", format!("({}, {})", px, py)));
    }

    let content = column(rows).spacing(6).padding(PAD * 2.0);

    container(scrollable(content).width(Length::Fill))
        .style(bar_style)
        .height(Length::Fill)
        .width(Length::Fixed(220.0))
        .into()
}
