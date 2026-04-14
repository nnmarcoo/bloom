use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use iced::alignment::{Horizontal, Vertical};
use iced::widget::canvas::{self, Canvas, Frame, Stroke};
use iced::widget::image::{self, FilterMethod};
use iced::widget::scrollable::{Direction, Scrollbar};
use iced::widget::tooltip::Position;
use iced::widget::{Space, button, column, container, row, scrollable, stack, text};
use iced::{
    Background, Color, Element, Font, Length, Point, Rectangle, Renderer, Theme, border, mouse,
};

use crate::app::Message;
use crate::gallery::Gallery;
use crate::styles::{PAD, bar_style, info_section_header_style, radius};
use crate::ui::{format_duration, with_tooltip_delay};
use crate::wgpu::view_program::ViewProgram;
use crate::widgets::histogram::Histogram;

const INFO_COLUMN_WIDTH: f32 = 220.0;

struct Crosshair {
    pixel_size: f32,
}

impl canvas::Program<Message> for Crosshair {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry<Renderer>> {
        let mut frame = Frame::new(renderer, bounds.size());
        let cx = bounds.width / 2.0;
        let cy = bounds.height / 2.0;
        let gap = self.pixel_size / 2.0;

        let arms = [
            (Point::new(0.0, cy), Point::new(cx - gap, cy)),
            (Point::new(cx + gap, cy), Point::new(bounds.width, cy)),
            (Point::new(cx, 0.0), Point::new(cx, cy - gap)),
            (Point::new(cx, cy + gap), Point::new(cx, bounds.height)),
        ];

        for (color, width) in [(Color::BLACK, 4.0_f32), (Color::WHITE, 2.0_f32)] {
            let stroke = Stroke {
                style: canvas::stroke::Style::Solid(color),
                width,
                line_cap: canvas::LineCap::Square,
                ..Stroke::default()
            };
            for (from, to) in arms {
                frame.stroke(&canvas::Path::line(from, to), stroke.clone());
            }
        }

        vec![frame.into_geometry()]
    }
}

fn section_header(label: &'static str, header_color: Color) -> Element<'static, Message> {
    let content = container(
        text(label)
            .size(11)
            .color(header_color)
            .font(Font::MONOSPACE),
    )
    .padding([2, 5])
    .width(Length::Fill)
    .align_x(Horizontal::Center)
    .align_y(Vertical::Center);

    button(content)
        .on_press(Message::ToggleInfoSection(label))
        .padding(0)
        .style(info_section_header_style)
        .width(Length::Fill)
        .into()
}

fn row_item<'a>(lbl: &'a str, val: impl ToString, muted: Color) -> Element<'a, Message> {
    row![
        text(lbl)
            .size(12)
            .color(muted)
            .font(Font::MONOSPACE)
            .width(Length::Fill),
        text(val.to_string())
            .size(12)
            .font(Font::MONOSPACE)
            .align_x(Horizontal::Right),
    ]
    .into()
}

fn color_row<'a>(rgba: [u8; 4], muted: Color) -> Element<'a, Message> {
    let [r, g, b, a] = rgba;
    let color = Color {
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
        text("RGBA").size(12).color(muted).font(Font::MONOSPACE),
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

pub fn view<'a>(
    path: Option<&'a Path>,
    gallery: &Gallery,
    program: &ViewProgram,
    theme: &Theme,
    info_collapsed: &HashSet<String>,
    pixel_preview_size: u32,
) -> Element<'a, Message> {
    let palette = theme.extended_palette();
    let muted = palette.background.base.text.scale_alpha(0.5);

    if program.image_size().is_none() {
        return container(
            text("No image loaded")
                .size(12)
                .color(muted)
                .font(Font::MONOSPACE),
        )
        .style(bar_style)
        .height(Length::Fill)
        .width(Length::Fixed(INFO_COLUMN_WIDTH))
        .align_x(Horizontal::Center)
        .align_y(Vertical::Center)
        .into();
    }

    let header_color = palette.background.base.text.scale_alpha(0.75);

    let mut first_section = true;
    let mut push_section = |rows: &mut Vec<Element<'a, Message>>,
                            label: &'static str,
                            section: Vec<Element<'a, Message>>| {
        if !section.is_empty() {
            if !first_section {
                rows.push(Space::new().height(PAD * 2.0).into());
            }
            first_section = false;
            let collapsed = info_collapsed.contains(label);
            let mut block = vec![section_header(label, header_color)];
            if !collapsed {
                block.extend(section);
            }
            rows.push(column(block).spacing(6).width(Length::Fill).into());
        }
    };

    let mut rows: Vec<Element<'a, Message>> = Vec::new();

    let mut file_rows: Vec<Element<'a, Message>> = Vec::new();
    if let Some(p) = path {
        if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
            let filename_row = row_item("Filename", truncate_filename(name, 18), muted);
            file_rows.push(if let Some(path_str) = p.to_str() {
                with_tooltip_delay(filename_row, path_str, Position::Right, Duration::ZERO)
            } else {
                filename_row
            });
        }
        if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
            file_rows.push(row_item("Format", ext.to_ascii_uppercase(), muted));
        }
    }
    let count = gallery.len();
    if count > 0 {
        file_rows.push(row_item(
            "In folder",
            format!("{} / {}", gallery.position() + 1, count),
            muted,
        ));
    }
    push_section(&mut rows, "FILE", file_rows);

    let mut image_rows: Vec<Element<'a, Message>> = Vec::new();
    if let Some((w, h)) = program.image_size() {
        image_rows.push(row_item("Dimensions", format!("{} x {}", w, h), muted));
        image_rows.push(row_item("Aspect ratio", aspect_ratio_str(w, h), muted));
    }
    image_rows.push(row_item(
        "Scale",
        format!("{:.0}%", program.scale() * 100.0),
        muted,
    ));
    if let Some(size) = gallery.file_size() {
        image_rows.push(row_item("File size", format_size(size), muted));
    }
    if let Some(bytes) = program.decoded_size_bytes() {
        image_rows.push(row_item("RAM", format_size(bytes as u64), muted));
    }
    if let Some(bytes) = program.vram_usage_bytes() {
        image_rows.push(row_item("VRAM", format_size(bytes as u64), muted));
    }
    push_section(&mut rows, "IMAGE", image_rows);

    let mut anim_rows: Vec<Element<'a, Message>> = Vec::new();
    if let Some((frame, total)) = program.animation_info() {
        anim_rows.push(row_item(
            "Frame",
            format!("{} / {}", frame + 1, total),
            muted,
        ));
    }
    if let Some((ts, dur)) = program
        .animation_timestamp()
        .zip(program.animation_duration())
    {
        anim_rows.push(row_item(
            "Time",
            format!("{} / {}", format_duration(ts), format_duration(dur)),
            muted,
        ));
    }
    push_section(&mut rows, "ANIMATION", anim_rows);

    let mut camera_rows: Vec<Element<'a, Message>> = Vec::new();
    if let Some(exif) = program.exif() {
        for (label, value) in [
            ("Make", &exif.make),
            ("Model", &exif.model),
            ("Date", &exif.datetime),
            ("Exposure", &exif.exposure_time),
            ("Aperture", &exif.f_number),
            ("ISO", &exif.iso),
            ("Focal len", &exif.focal_length),
            ("GPS", &exif.gps),
        ] {
            if let Some(v) = value {
                camera_rows.push(row_item(label, v, muted));
            }
        }
    }
    push_section(&mut rows, "EXIF", camera_rows);

    let mut cursor_rows: Vec<Element<'a, Message>> = Vec::new();
    if let Some((px, py, rgba)) = program.cursor_info() {
        if let Some(pixels) = program.cursor_pixels(pixel_preview_size) {
            let display_size = INFO_COLUMN_WIDTH - PAD * 4.0;
            let pixel_size = display_size / pixel_preview_size as f32;
            let handle = image::Handle::from_rgba(pixel_preview_size, pixel_preview_size, pixels);
            cursor_rows.push(
                stack![
                    image::Image::new(handle)
                        .filter_method(FilterMethod::Nearest)
                        .width(Length::Fill)
                        .height(Length::Fixed(display_size)),
                    Canvas::new(Crosshair { pixel_size })
                        .width(Length::Fixed(display_size))
                        .height(Length::Fixed(display_size)),
                ]
                .into(),
            );
        }
        cursor_rows.push(color_row(rgba, muted));
        cursor_rows.push(row_item("Pixel", format!("({}, {})", px, py), muted));
    }
    push_section(&mut rows, "CURSOR", cursor_rows);

    if let Some(histogram) = program.histogram() {
        rows.push(Space::new().height(PAD * 2.0).into());
        rows.push(
            Histogram::new(histogram.0, histogram.1, histogram.2)
                .height(142.0)
                .into(),
        );
    }

    let content = column(rows).padding(PAD * 2.0);

    container(
        scrollable(content)
            .width(Length::Fill)
            .direction(Direction::Vertical(
                Scrollbar::new().width(4).scroller_width(4),
            )),
    )
    .style(bar_style)
    .height(Length::Fill)
    .width(Length::Fixed(INFO_COLUMN_WIDTH))
    .into()
}
