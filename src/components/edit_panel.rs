use iced::widget::tooltip::Position;
use iced::widget::{Space, container, row};
use iced::{Element, Length};

use crate::app::{Message, Tool};
use crate::components::modifier_stack;
use crate::modifiers::Modifier;
use crate::styles::{EDIT_PANEL_WIDTH, PAD, bar_style, panel_divider_style};
use crate::ui::{svg_button_active, svg_button_plain, with_tooltip};

fn tool_button<'a>(icon: &'static [u8], tool: Tool, selected_tool: &Tool) -> Element<'a, Message> {
    let msg = Message::SelectTool(tool.clone());
    if &tool == selected_tool {
        svg_button_active(icon, msg)
    } else {
        svg_button_plain(icon, msg)
    }
}

pub fn view<'a>(
    selected_tool: &Tool,
    modifiers: &'a [Modifier],
    dragging_modifier: Option<usize>,
    drag_hover_target: Option<usize>,
) -> Element<'a, Message> {
    use iced::widget::column;

    let tool_strip = container(
        column![
            with_tooltip(
                tool_button(
                    include_bytes!("../../assets/icons/cursor.svg"),
                    Tool::Select,
                    selected_tool,
                ),
                "Select",
                Position::Left,
            ),
            with_tooltip(
                tool_button(
                    include_bytes!("../../assets/icons/crop.svg"),
                    Tool::Crop,
                    selected_tool,
                ),
                "Crop",
                Position::Left,
            ),
            with_tooltip(
                tool_button(
                    include_bytes!("../../assets/icons/pencil.svg"),
                    Tool::Draw,
                    selected_tool,
                ),
                "Draw",
                Position::Left,
            ),
            with_tooltip(
                tool_button(
                    include_bytes!("../../assets/icons/text.svg"),
                    Tool::Text,
                    selected_tool,
                ),
                "Text",
                Position::Left,
            ),
            with_tooltip(
                tool_button(
                    include_bytes!("../../assets/icons/pen.svg"),
                    Tool::Mask,
                    selected_tool,
                ),
                "Mask",
                Position::Left,
            ),
        ]
        .spacing(2),
    )
    .padding(PAD)
    .width(Length::Shrink)
    .height(Length::Fill);

    let divider = container(Space::new())
        .width(Length::Fixed(2.0))
        .height(Length::Fill)
        .style(panel_divider_style);

    let stack = container(modifier_stack::view(
        modifiers,
        dragging_modifier,
        drag_hover_target,
    ))
    .width(Length::Fill)
    .height(Length::Fill);

    container(row![tool_strip, divider, stack].height(Length::Fill))
        .style(bar_style)
        .height(Length::Fill)
        .width(Length::Fixed(EDIT_PANEL_WIDTH))
        .into()
}
