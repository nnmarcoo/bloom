use iced::alignment::Vertical;
use iced::widget::container;
use iced::widget::row;
use iced::widget::tooltip::Position;
use iced::{Element, Length};

use crate::app::Message;
use crate::styles::{BAR_HEIGHT, PAD, bar_style};
use crate::ui::{svg_button, with_tooltip};
use crate::widgets::timeline::Timeline;

pub fn view<'a>(
    total_frames: usize,
    position: f32,
    playing: bool,
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

    container(
        row![
            controls,
            Timeline::new(playing, position, total_frames, Message::FrameSeek)
                .on_drag_start(Message::TimelineScrubStart)
                .on_drag_end(Message::TimelineScrubEnd),
        ]
        .height(Length::Fixed(BAR_HEIGHT))
        .width(Length::Fill)
        .align_y(Vertical::Center)
        .spacing(PAD),
    )
    .padding([0.0, PAD])
    .style(bar_style)
    .into()
}
