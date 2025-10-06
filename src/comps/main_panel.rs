use crate::app::Message;
use iced::{
    ContentFit, Element,
    Length::Fill,
    widget::{
        container,
        image::{self, FilterMethod, Viewer},
    },
};

pub fn data_panel<'a>() -> Element<'a, Message> {
    const IMAGE_BYTES: &[u8] = include_bytes!("../assets/debug.jpg");
    let handle = image::Handle::from_bytes(IMAGE_BYTES);

    let viewer = Viewer::new(handle)
        .filter_method(FilterMethod::Nearest)
        .content_fit(ContentFit::ScaleDown)
        .scale_step(0.15)
        .width(Fill)
        .height(Fill);

    container(viewer).into()
}
