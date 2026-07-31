use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;
use std::time::Duration;

use iced::Element;
use iced::alignment::Horizontal;
use iced::widget::{column, text};
use iced::{Font, Length};

use crate::app::{EditMsg, Message};
use crate::modifiers::{MediaTiming, ModifierImpl, ModifierParam, ViewCtx};
use crate::ui::format_duration;
use crate::widgets::value_slider::Fmt;

use super::{finish, value_row};

// a trim shorter than this is degenerate; keeps the range non-empty
pub const MIN_SPAN: Duration = Duration::from_millis(1);

#[derive(Debug, Clone, Default)]
pub struct Trim {
    pub start: Duration,
    pub end: Option<Duration>,
}

impl Trim {
    pub fn is_full(&self) -> bool {
        self.start.is_zero() && self.end.is_none()
    }

    // an unset end means "to the end of whatever media is loaded"
    pub fn end_or(&self, duration: Duration) -> Duration {
        self.end.unwrap_or(duration).min(duration)
    }

    pub fn resolve(&self, duration: Duration) -> (Duration, Duration) {
        let duration = duration.max(MIN_SPAN);
        let end = self.end_or(duration).max(MIN_SPAN);
        let start = self.start.min(end - MIN_SPAN);
        (start, end)
    }

    pub fn set_start(&mut self, v: Duration, duration: Duration) {
        let end = self.end_or(duration.max(MIN_SPAN)).max(MIN_SPAN);
        self.start = v.min(end - MIN_SPAN);
    }

    pub fn set_end(&mut self, v: Duration, duration: Duration) {
        self.end = Some(v.max(self.start + MIN_SPAN).min(duration.max(MIN_SPAN)));
    }
}

fn secs_row<'a>(
    label: &'a str,
    value: Duration,
    max: Duration,
    on_change: impl Fn(f32) -> Message + 'static,
) -> Element<'a, Message> {
    value_row(
        label,
        value.as_secs_f32(),
        0.0..=max.as_secs_f32().max(0.001),
        0.001,
        Fmt::num(3).suffix("s"),
        on_change,
    )
}

fn readout<'a>(start: Duration, end: Duration, timing: MediaTiming) -> Element<'a, Message> {
    let label = if timing.frame_count > 0 {
        let (a, b) = (timing.frame_at(start), timing.frame_at(end));
        format!(
            "{} \u{2013} {}\nframe {} \u{2013} {} ({} kept)",
            format_duration(start),
            format_duration(end),
            a,
            b,
            b.saturating_sub(a),
        )
    } else {
        format!(
            "{} \u{2013} {}\n{} kept",
            format_duration(start),
            format_duration(end),
            format_duration(end.saturating_sub(start)),
        )
    };
    text(label)
        .size(9)
        .font(Font::MONOSPACE)
        .width(Length::Fill)
        .align_x(Horizontal::Center)
        .into()
}

impl ModifierImpl for Trim {
    fn name(&self) -> &'static str {
        "Trim"
    }

    fn has_effect(&self) -> bool {
        false
    }

    fn apply_param(&mut self, param: ModifierParam, _img_size: Option<(u32, u32)>) {
        match param {
            ModifierParam::TrimStart(secs, duration) => {
                self.set_start(Duration::from_secs_f32(secs.max(0.0)), duration);
            }
            ModifierParam::TrimEnd(secs, duration) => {
                self.set_end(Duration::from_secs_f32(secs.max(0.0)), duration);
            }
            _ => {}
        }
    }

    fn hash(&self, hasher: &mut DefaultHasher) {
        26u8.hash(hasher);
        self.start.hash(hasher);
        self.end.hash(hasher);
    }

    fn view(&self, index: usize, ctx: ViewCtx) -> Element<'_, Message> {
        let timing = ctx.timing.unwrap_or_default();
        let duration = timing.duration;
        let (start, end) = self.resolve(duration);

        finish(column![
            readout(start, end, timing),
            secs_row("Start", start, duration, move |v| EditMsg::Update(
                index,
                ModifierParam::TrimStart(v, duration)
            )
            .into()),
            secs_row("End", end, duration, move |v| EditMsg::Update(
                index,
                ModifierParam::TrimEnd(v, duration)
            )
            .into()),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUR: Duration = Duration::from_secs(10);

    #[test]
    fn handles_cannot_cross() {
        let mut t = Trim::default();
        t.set_end(Duration::from_secs(4), DUR);
        t.set_start(Duration::from_secs(9), DUR);
        let (start, end) = t.resolve(DUR);
        assert!(start + MIN_SPAN <= end, "{start:?} .. {end:?}");

        let mut t = Trim::default();
        t.set_start(Duration::from_secs(8), DUR);
        t.set_end(Duration::from_secs(2), DUR);
        let (start, end) = t.resolve(DUR);
        assert!(start + MIN_SPAN <= end, "{start:?} .. {end:?}");
    }

    #[test]
    fn full_range_is_a_no_op() {
        assert!(Trim::default().is_full());
        assert_eq!(Trim::default().resolve(DUR), (Duration::ZERO, DUR));
    }

    #[test]
    fn open_end_follows_the_loaded_media() {
        let mut t = Trim::default();
        t.set_start(Duration::from_secs(1), DUR);
        assert!(t.end.is_none());
        let (start, end) = t.resolve(Duration::from_secs(4));
        assert_eq!(
            (start, end),
            (Duration::from_secs(1), Duration::from_secs(4))
        );
    }

    #[test]
    fn range_stays_valid_on_shorter_media() {
        let mut t = Trim::default();
        t.set_start(Duration::from_secs(6), DUR);
        t.set_end(Duration::from_secs(9), DUR);
        let short = Duration::from_secs(2);
        let (start, end) = t.resolve(short);
        assert!(start < end, "{start:?} .. {end:?}");
        assert!(end <= short, "end {end:?} exceeds media {short:?}");
    }

    #[test]
    fn frame_at_maps_both_ends() {
        let timing = MediaTiming {
            duration: DUR,
            frame_count: 100,
        };
        assert_eq!(timing.frame_at(Duration::ZERO), 0);
        assert_eq!(timing.frame_at(DUR), 100);
        assert_eq!(timing.frame_at(Duration::from_secs(5)), 50);
    }

    #[test]
    fn frame_at_is_safe_without_timing() {
        assert_eq!(
            MediaTiming::default().frame_at(Duration::from_secs(1)),
            0,
            "unknown frame count must not divide by zero"
        );
    }
}
