use iced::widget::tooltip::Position;
use iced::widget::{Space, container, row};
use iced::{Element, Length};

use crate::app::{EditMsg, Message, Tool};
use crate::components::modifier_stack;
use crate::keybinds::{Action, Keymap};
use crate::modifiers::Modifier;
use crate::styles::{EDIT_PANEL_WIDTH, PAD, bar_style, panel_divider_style};
use crate::ui::{svg_button_active, svg_button_plain, with_tooltip_key};

fn tool_button<'a>(icon: &'static [u8], tool: Tool, selected_tool: &Tool) -> Element<'a, Message> {
    let msg = EditMsg::SelectTool(tool.clone()).into();
    if &tool == selected_tool {
        svg_button_active(icon, msg)
    } else {
        svg_button_plain(icon, msg)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn view<'a>(
    selected_tool: &Tool,
    keymap: &Keymap,
    modifiers: &'a [Modifier],
    active_modifier: Option<usize>,
    dragging_modifier: Option<usize>,
    drag_hover_target: Option<usize>,
    image_size: Option<(u32, u32)>,
    rotation: u8,
) -> Element<'a, Message> {
    use iced::widget::column;

    let tool_strip = container(
        column![
            with_tooltip_key(
                tool_button(
                    include_bytes!("../../assets/icons/cursor.svg"),
                    Tool::Select,
                    selected_tool,
                ),
                "Select",
                Position::Left,
                keymap,
                Action::ToolSelect,
            ),
            with_tooltip_key(
                tool_button(
                    include_bytes!("../../assets/icons/crop.svg"),
                    Tool::Crop,
                    selected_tool,
                ),
                "Crop",
                Position::Left,
                keymap,
                Action::ToolCrop,
            ),
            with_tooltip_key(
                tool_button(
                    include_bytes!("../../assets/icons/text.svg"),
                    Tool::Text,
                    selected_tool,
                ),
                "Text",
                Position::Left,
                keymap,
                Action::ToolText,
            ),
            with_tooltip_key(
                tool_button(
                    include_bytes!("../../assets/icons/pencil.svg"),
                    Tool::Draw,
                    selected_tool,
                ),
                "Draw",
                Position::Left,
                keymap,
                Action::ToolDraw,
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
        active_modifier,
        dragging_modifier,
        drag_hover_target,
        image_size,
        rotation,
    ))
    .width(Length::Fill)
    .height(Length::Fill);

    container(row![tool_strip, divider, stack].height(Length::Fill))
        .style(bar_style)
        .height(Length::Fill)
        .width(Length::Fixed(EDIT_PANEL_WIDTH))
        .into()
}
