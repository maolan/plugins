use maolan_baseview::iced::{
    Alignment, Element, Length,
    widget::{column, container, text},
};
use maolan_widgets::arch_slider::arch_slider;
use maolan_widgets::meters;
use maolan_widgets::slider::Slider as VerticalSliderWidget;
use maolan_widgets::ticks;

pub const FADER_MIN_DB: f32 = -90.0;
pub const FADER_MAX_DB: f32 = 20.0;
const SIDE_FADER_WIDTH: f32 = 50.0;
const SIDE_TICKS_WIDTH: f32 = 25.0;
const SIDE_COLUMN_WIDTH: f32 = SIDE_FADER_WIDTH + SIDE_TICKS_WIDTH;

pub struct SmallKnob {
    pub label: String,
    pub value: f32,
    pub range: std::ops::RangeInclusive<f32>,
    pub default: f32,
    pub step: f32,
    pub value_text: String,
}

pub fn small_knob<Message: Clone + 'static>(
    knob: SmallKnob,
    on_change: impl Fn(f32) -> Message + 'static,
    on_release: Message,
) -> Element<'static, Message> {
    let min = *knob.range.start();
    let max = *knob.range.end();
    let slider = arch_slider(knob.range, knob.value.clamp(min, max), on_change)
        .step(knob.step)
        .double_click_reset(knob.default)
        .on_release(on_release)
        .fill_from_start()
        .width(Length::Fixed(41.0))
        .height(Length::Fixed(41.0));

    container(
        column![
            text(knob.label).size(11),
            slider,
            text(knob.value_text).size(10)
        ]
        .spacing(2)
        .align_x(Alignment::Center),
    )
    .width(Length::Fixed(SIDE_FADER_WIDTH))
    .into()
}

pub struct VerticalSlider {
    pub value: f32,
    pub range: std::ops::RangeInclusive<f32>,
    pub default: f32,
    pub step: f32,
    pub value_text: String,
}

pub fn vertical_slider<Message: Clone + 'static>(
    fader: VerticalSlider,
    on_change: impl Fn(f32) -> Message + 'static,
    on_release: Message,
) -> Element<'static, Message> {
    let min = *fader.range.start();
    let max = *fader.range.end();
    let slider = VerticalSliderWidget::new(fader.range, fader.value.clamp(min, max), on_change)
        .step(fader.step)
        .double_click_reset(fader.default)
        .on_release(on_release)
        .width(Length::Fixed(14.0))
        .height(Length::Fill);

    container(
        column![slider, text(fader.value_text).size(10)]
            .spacing(4)
            .height(Length::Fill)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(SIDE_FADER_WIDTH))
    .height(Length::Fill)
    .into()
}

pub fn vertical_ticks<Message: 'static>() -> Element<'static, Message> {
    column![
        ticks::ticks(FADER_MIN_DB..=FADER_MAX_DB, 1.0),
        text("").size(10)
    ]
    .spacing(4)
    .height(Length::Fill)
    .into()
}

pub fn vu_meter<Message: 'static>(
    channels: usize,
    levels_db: [f32; 2],
) -> Element<'static, Message> {
    let channels = channels.clamp(1, 2);
    let peak_db = levels_db
        .iter()
        .take(channels)
        .copied()
        .fold(FADER_MIN_DB, f32::max)
        .clamp(FADER_MIN_DB, FADER_MAX_DB);
    let readout = if peak_db <= FADER_MIN_DB {
        "-inf dB".to_string()
    } else {
        format!("{peak_db:.1} dB")
    };

    container(
        column![
            container(meters::meters(channels, &levels_db, 1.0))
                .width(Length::Fixed(meters::total_width(channels)))
                .height(Length::Fill),
            text(readout).size(10),
        ]
        .spacing(4)
        .height(Length::Fill)
        .align_x(Alignment::Center),
    )
    .width(Length::Fixed(SIDE_COLUMN_WIDTH))
    .height(Length::Fill)
    .into()
}
