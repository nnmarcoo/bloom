use std::time::{Duration, Instant};

use iced::widget::svg::Handle;
use iced::widget::{button, column, container, row, svg, text};
use iced::{Alignment, Color, Element, Length, Padding};

use crate::app::Message;
use crate::styles::{BUTTON_SIZE, PAD, toast_container_style, toast_dismiss_style};
use crate::widgets::slide_in::SlideIn;

const ANIM_SECS: f32 = 0.25;
const TOAST_WIDTH: f32 = 300.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationKind {
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct Notification {
    pub kind: NotificationKind,
    pub message: String,
}

impl Notification {
    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            kind: NotificationKind::Warning,
            message: message.into(),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            kind: NotificationKind::Error,
            message: message.into(),
        }
    }

    fn timeout(&self) -> Option<Duration> {
        match self.kind {
            NotificationKind::Warning => Some(Duration::from_secs(8)),
            NotificationKind::Error => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NotificationEntry {
    pub notification: Notification,
    pub pushed_at: Instant,
    pub dismissing_at: Option<Instant>,
}

impl NotificationEntry {
    pub fn new(notification: Notification) -> Self {
        Self {
            notification,
            pushed_at: Instant::now(),
            dismissing_at: None,
        }
    }

    pub fn is_animating(&self) -> bool {
        self.pushed_at.elapsed().as_secs_f32() < ANIM_SECS || self.dismissing_at.is_some()
    }

    pub fn is_gone(&self, now: Instant) -> bool {
        self.dismissing_at
            .is_some_and(|d| now.duration_since(d).as_secs_f32() >= ANIM_SECS)
    }

    pub fn expire_if_due(&mut self, now: Instant) {
        if self.dismissing_at.is_none() {
            if let Some(timeout) = self.notification.timeout() {
                if now.duration_since(self.pushed_at) >= timeout {
                    self.dismissing_at = Some(now);
                }
            }
        }
    }

    fn alpha(&self, now: Instant) -> f32 {
        let t_in = (now.duration_since(self.pushed_at).as_secs_f32() / ANIM_SECS).min(1.0);
        let fade_in = ease_out_cubic(t_in);
        let fade_out = self.dismissing_at.map_or(1.0, |d| {
            let t_out = (now.duration_since(d).as_secs_f32() / ANIM_SECS).min(1.0);
            1.0 - ease_in_cubic(t_out)
        });
        fade_in * fade_out
    }
}

fn ease_out_cubic(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn ease_in_cubic(t: f32) -> f32 {
    t.powi(3)
}

pub fn view<'a>(notifications: &'a [NotificationEntry]) -> Element<'a, Message> {
    if notifications.is_empty() {
        return iced::widget::Space::new().into();
    }

    let now = Instant::now();
    let toasts: Vec<Element<'a, Message>> = notifications
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let alpha = entry.alpha(now);
            let offset_x = (1.0 - alpha) * (TOAST_WIDTH + PAD * 2.0);
            SlideIn::new(toast_view(i, &entry.notification, alpha), offset_x).into()
        })
        .collect();

    container(column(toasts).spacing(PAD).align_x(Alignment::End))
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Right)
        .align_y(iced::alignment::Vertical::Bottom)
        .padding(Padding {
            top: 0.0,
            right: PAD * 2.0,
            bottom: PAD * 2.0,
            left: 0.0,
        })
        .into()
}

fn toast_view(index: usize, n: &Notification, alpha: f32) -> Element<'_, Message> {
    let (icon_bytes, accent_fn): (&'static [u8], fn(&iced::Theme) -> Color) = match n.kind {
        NotificationKind::Warning => (
            include_bytes!("../../assets/icons/error.svg"),
            |t: &iced::Theme| t.extended_palette().warning.base.color,
        ),
        NotificationKind::Error => (
            include_bytes!("../../assets/icons/error.svg"),
            |t: &iced::Theme| t.extended_palette().danger.base.color,
        ),
    };

    let icon = svg(Handle::from_memory(icon_bytes))
        .style(move |theme, _| iced::widget::svg::Style {
            color: Some(accent_fn(theme).scale_alpha(alpha)),
        })
        .width(Length::Fixed(BUTTON_SIZE))
        .height(Length::Fixed(BUTTON_SIZE));

    let dismiss = button(
        svg(Handle::from_memory(include_bytes!(
            "../../assets/icons/close.svg"
        )))
        .style(move |theme: &iced::Theme, _| iced::widget::svg::Style {
            color: Some(
                theme
                    .extended_palette()
                    .background
                    .base
                    .text
                    .scale_alpha(0.6 * alpha),
            ),
        })
        .width(Length::Fixed(BUTTON_SIZE))
        .height(Length::Fixed(BUTTON_SIZE)),
    )
    .on_press(Message::DismissNotification(index))
    .style(move |theme, status| toast_dismiss_style(theme, status, alpha))
    .padding(PAD);

    container(
        row![
            icon,
            text(n.message.as_str()).size(13.0).width(Length::Fill),
            dismiss,
        ]
        .spacing(PAD)
        .width(Length::Fill)
        .align_y(Alignment::Center),
    )
    .padding([PAD, PAD * 1.5])
    .style(move |theme| toast_container_style(theme, accent_fn(theme), alpha))
    .width(Length::Fixed(TOAST_WIDTH))
    .into()
}
