use std::time::Duration;

use iced::alignment::Vertical;
use iced::widget::tooltip::Position;
use iced::widget::{container, row, slider, text};
use iced::{Element, Font, Length};

use crate::app::Message;
use crate::styles::{BAR_HEIGHT, PAD, bar_style};
use crate::ui::{format_duration, svg_button, with_tooltip};
use crate::widgets::timeline::Timeline;

pub fn view<'a>(
    total_frames: usize,
    position: f32,
    playing: bool,
    timestamp: Option<(Duration, Duration)>,
    volume: Option<f32>,
    muted: bool,
) -> Element<'a, Message> {
    let (play_pause_icon, play_pause_tooltip): (&'static [u8], &str) = if playing {
        (include_bytes!("../../assets/icons/pause.svg"), "Pause")
    } else {
        (include_bytes!("../../assets/icons/play.svg"), "Play")
    };

    let controls = row![
        with_tooltip(
            svg_button(
                include_bytes!("../../assets/icons/first.svg"),
                Message::FrameFirst
            ),
            "First frame",
            Position::Top,
        ),
        with_tooltip(
            svg_button(
                include_bytes!("../../assets/icons/left.svg"),
                Message::FramePrev
            ),
            "Previous frame",
            Position::Top,
        ),
        with_tooltip(
            svg_button(play_pause_icon, Message::TogglePlayback),
            play_pause_tooltip,
            Position::Top,
        ),
        with_tooltip(
            svg_button(
                include_bytes!("../../assets/icons/right.svg"),
                Message::FrameNext
            ),
            "Next frame",
            Position::Top,
        ),
        with_tooltip(
            svg_button(
                include_bytes!("../../assets/icons/last.svg"),
                Message::FrameLast
            ),
            "Last frame",
            Position::Top,
        ),
    ]
    .align_y(Vertical::Center)
    .spacing(PAD);

    let timeline = Timeline::new(playing, position, total_frames, Message::FrameSeek)
        .on_drag_start(Message::TimelineScrubStart)
        .on_drag_end(Message::TimelineScrubEnd);

    let label = timestamp
        .map(|(ts, dur)| format!("{} – {}", format_duration(ts), format_duration(dur)))
        .unwrap_or_default();

    let label_widget = container(text(label).size(12).font(Font::MONOSPACE))
        .padding([2.0, PAD])
        .style(|theme: &iced::Theme| {
            let palette = theme.extended_palette();
            container::Style {
                text_color: Some(palette.background.base.text),
                background: Some(iced::Background::Color(palette.background.base.color)),
                border: iced::Border {
                    radius: crate::styles::radius().into(),
                    ..iced::Border::default()
                },
                ..Default::default()
            }
        });

    let mut bar = row![controls, timeline, label_widget]
        .height(Length::Fixed(BAR_HEIGHT))
        .width(Length::Fill)
        .align_y(Vertical::Center)
        .spacing(PAD);

    if let Some(level) = volume {
        let (icon, tooltip): (&'static [u8], &str) = if muted || level <= 0.0 {
            (
                include_bytes!("../../assets/icons/volume-mute.svg"),
                "Unmute",
            )
        } else {
            (include_bytes!("../../assets/icons/volume.svg"), "Mute")
        };
        let shown = if muted { 0.0 } else { level };
        let volume_control = row![
            with_tooltip(
                svg_button(icon, Message::ToggleMute),
                tooltip,
                Position::Top,
            ),
            slider(0.0..=2.0, shown, Message::SetVolume)
                .step(0.01)
                .width(Length::Fixed(90.0)),
        ]
        .align_y(Vertical::Center)
        .spacing(PAD);
        bar = bar.push(volume_control);
    }

    container(bar).padding([0.0, PAD]).style(bar_style).into()
}
