use iced::alignment::Vertical;
use iced::widget::container;
use iced::widget::tooltip::Position;
use iced::widget::{row, text};
use iced::{Element, Length};

use crate::app::Message;
use crate::styles::{BAR_HEIGHT, PAD, bar_style};
use crate::ui::{svg_button, with_tooltip};
use crate::widgets::timeline::Timeline;

pub fn view<'a>(frame: usize, total_frames: usize, playing: bool) -> Element<'a, Message> {
    let (play_pause_icon, play_pause_tooltip): (&'static [u8], &str) = if playing {
        (include_bytes!("../../assets/icons/pause.svg"), "Pause")
    } else {
        (include_bytes!("../../assets/icons/play.svg"), "Play")
    };

    let controls = row![
        with_tooltip(
            svg_button(
                include_bytes!("../../assets/icons/first.svg"),
                Message::Noop
            ),
            "First frame",
            Position::Top,
        ),
        with_tooltip(
            svg_button(include_bytes!("../../assets/icons/left.svg"), Message::Noop),
            "Previous frame",
            Position::Top,
        ),
        with_tooltip(
            svg_button(play_pause_icon, Message::Noop),
            play_pause_tooltip,
            Position::Top,
        ),
        with_tooltip(
            svg_button(
                include_bytes!("../../assets/icons/right.svg"),
                Message::Noop
            ),
            "Next frame",
            Position::Top,
        ),
        with_tooltip(
            svg_button(include_bytes!("../../assets/icons/last.svg"), Message::Noop),
            "Last frame",
            Position::Top,
        ),
    ]
    .align_y(Vertical::Center)
    .spacing(PAD);

    container(
        row![
            controls,
            Timeline::new(frame, total_frames, |_f| Message::Noop),
            text(format!("{} / {}", frame + 1, total_frames)).size(12),
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
