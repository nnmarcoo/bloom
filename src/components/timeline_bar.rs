use std::time::Duration;

use iced::alignment::Vertical;
use iced::widget::tooltip::Position;
use iced::widget::{container, row, text};
use iced::{Element, Font, Length};

use crate::app::{Message, TransportMsg};
use crate::styles::{BAR_HEIGHT, PAD, bar_style};
use crate::ui::{format_duration, svg_button, with_tooltip};
use crate::widgets::timeline::Timeline;
use crate::widgets::value_slider::{Fmt, ValueSlider};

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
                TransportMsg::FrameFirst.into()
            ),
            "First frame",
            Position::Top,
        ),
        with_tooltip(
            svg_button(
                include_bytes!("../../assets/icons/left.svg"),
                TransportMsg::FramePrev.into()
            ),
            "Previous frame",
            Position::Top,
        ),
        with_tooltip(
            svg_button(play_pause_icon, TransportMsg::TogglePlayback.into()),
            play_pause_tooltip,
            Position::Top,
        ),
        with_tooltip(
            svg_button(
                include_bytes!("../../assets/icons/right.svg"),
                TransportMsg::FrameNext.into()
            ),
            "Next frame",
            Position::Top,
        ),
        with_tooltip(
            svg_button(
                include_bytes!("../../assets/icons/last.svg"),
                TransportMsg::FrameLast.into()
            ),
            "Last frame",
            Position::Top,
        ),
    ]
    .align_y(Vertical::Center)
    .spacing(PAD);

    let timeline = Timeline::new(playing, position, total_frames, |i| {
        TransportMsg::FrameSeek(i).into()
    })
    .on_drag_start(TransportMsg::ScrubStart.into())
    .on_drag_end(TransportMsg::ScrubEnd.into());

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
                svg_button(icon, TransportMsg::ToggleMute.into()),
                tooltip,
                Position::Top,
            ),
            container(
                ValueSlider::new(shown * 100.0, 0.0..=200.0, |v| TransportMsg::SetVolume(
                    v / 100.0
                )
                .into())
                .step(1.0)
                .format(Fmt::num(0).suffix("%"))
                .on_change_end(TransportMsg::CommitVolume.into())
            )
            .width(Length::Fixed(90.0)),
        ]
        .align_y(Vertical::Center)
        .spacing(PAD);
        bar = bar.push(volume_control);
    }

    container(bar).padding([0.0, PAD]).style(bar_style).into()
}
