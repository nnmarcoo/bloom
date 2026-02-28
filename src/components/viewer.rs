use iced::{
    Center, Element, Length,
    widget::{column, container, shader, stack, text},
};

use crate::{
    app::Message, styles::spinner_bg_style, wgpu::view_program::ViewProgram,
    widgets::loading_spinner::Circular,
};

pub fn view<'a>(program: ViewProgram, loading: Option<&'a str>) -> Element<'a, Message> {
    let base = shader(program).height(Length::Fill).width(Length::Fill);

    if let Some(filename) = loading {
        let overlay = container(
            container(
                column![
                    Circular::<iced::Theme>::new().size(36.0).bar_height(4.0),
                    text(filename).size(12),
                ]
                .spacing(8)
                .align_x(Center),
            )
            .padding(16)
            .style(spinner_bg_style),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Center)
        .align_y(Center);

        stack![base, overlay]
            .height(Length::Fill)
            .width(Length::Fill)
            .into()
    } else {
        base.into()
    }
}
