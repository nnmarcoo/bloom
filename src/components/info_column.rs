use std::path::Path;

use iced::alignment::Horizontal;
use iced::widget::tooltip::Position;
use iced::widget::{column, container, row, scrollable, text, tooltip};
use iced::{Element, Font, Length};

use crate::app::Message;
use crate::gallery::Gallery;
use crate::styles::{PAD, TOOLTIP_DELAY, bar_style};
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

pub fn view<'a>(
    path: Option<&'a Path>,
    gallery: &Gallery,
    program: &ViewProgram,
) -> Element<'a, Message> {
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
    }

    let count = gallery.len();
    if count > 0 {
        rows.push(row_item(
            "In folder",
            format!("{} / {}", gallery.position() + 1, count),
        ));
    }

    if let Some((w, h)) = program.image_size() {
        rows.push(row_item("Dimensions", format!("{} × {}", w, h)));
    }

    rows.push(row_item(
        "Scale",
        format!("{:.0}%", program.scale() * 100.0),
    ));

    if let Some(size) = path
        .and_then(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
    {
        rows.push(row_item("File size", format_size(size)));
    }

    if let Some((frame, total)) = program.animation_info() {
        rows.push(row_item("Frame", format!("{} / {}", frame + 1, total)));
    }

    let content = column(rows).spacing(6).padding(PAD * 2.0);

    container(scrollable(content).width(Length::Fill))
        .style(bar_style)
        .height(Length::Fill)
        .width(Length::Fixed(220.0))
        .into()
}
