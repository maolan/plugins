use std::{
    ffi::CStr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use crate::eq::common::gui::{AnyWindowHandle, ParentWindowHandle, is_api_supported};
use crate::eq::common::params::ParamIdExt;
use crate::eq::common::plugin::SharedState;
use crate::eq::graphic::params::{PARAMS, ParamId};
use maolan_baseview::iced::{
    Alignment, Element, Length, Task, Theme,
    alignment::{Horizontal, Vertical},
    widget::{column, container, row, scrollable, text},
};
use maolan_widgets::meters::meters;
use maolan_widgets::slider::slider;

pub const EDITOR_WIDTH: u32 = 900;
pub const EDITOR_HEIGHT: u32 = 200;

#[derive(Debug, Clone)]
pub enum Message {
    SetParam(ParamId, f32),
    SetBoolParam(ParamId, bool),
}

struct State {
    shared: Arc<SharedState<ParamId>>,
}

fn init(shared: Arc<SharedState<ParamId>>) -> (State, Task<Message>) {
    (State { shared }, Task::none())
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::SetParam(id, value) => state.shared.set_param(id, value as f64),
        Message::SetBoolParam(id, value) => {
            state.shared.set_param(id, if value { 1.0 } else { 0.0 })
        }
    }
    Task::none()
}

fn view(state: &State) -> Element<'_, Message> {
    let p = |id: ParamId| state.shared.params.get(id) as f32;

    let mut content = column![].spacing(10).align_x(Alignment::Start);

    let ch = state
        .shared
        .channels
        .load(std::sync::atomic::Ordering::Relaxed)
        .clamp(1, 2) as usize;
    let input_levels_db: Vec<f32> = if ch == 1 {
        vec![state.shared.input_level_left_db()]
    } else {
        vec![
            state.shared.input_level_left_db(),
            state.shared.input_level_right_db(),
        ]
    };
    let output_levels_db: Vec<f32> = if ch == 1 {
        vec![state.shared.output_level_left_db()]
    } else {
        vec![
            state.shared.output_level_left_db(),
            state.shared.output_level_right_db(),
        ]
    };

    let mut graphic_bands = row![].spacing(10);
    for i in 0..32 {
        graphic_bands = graphic_bands.push(graphic_band(state, i));
    }

    let mut graphic_row =
        row![container(meters(ch, &input_levels_db, 150.0)).height(Length::Fill)].spacing(10);
    graphic_row = graphic_row.push(
        scrollable(graphic_bands)
            .direction(scrollable::Direction::Horizontal(Default::default()))
            .height(Length::Fixed(150.0)),
    );
    graphic_row =
        graphic_row.push(container(meters(ch, &output_levels_db, 150.0)).height(Length::Fill));

    let parameters = column![graphic_row].spacing(10).align_x(Alignment::Center);

    content = content.push(
        row![
            gain_slider(ParamId::InputGain, p(ParamId::InputGain)),
            parameters,
            gain_slider(ParamId::OutputGain, p(ParamId::OutputGain)),
        ]
        .spacing(14)
        .align_y(Alignment::Start),
    );

    container(content)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Horizontal::Left)
        .align_y(Vertical::Top)
        .into()
}

fn graphic_band(state: &State, index: usize) -> Element<'_, Message> {
    let p = |id: ParamId| state.shared.params.get(id) as f32;
    let id = ParamId::graphic_gain(index);

    vertical_knob(id, p(id), 0.1)
}

fn theme(_state: &State) -> Theme {
    Theme::TokyoNight
}

fn vertical_knob(id: ParamId, value: f32, step: f32) -> Element<'static, Message> {
    let def = PARAMS[id.as_index()];
    let slider = slider(def.min as f32..=def.max as f32, value, move |v| {
        Message::SetParam(id, v)
    })
    .step(step)
    .double_click_reset(def.default as f32)
    .width(Length::Fixed(10.0))
    .height(Length::Fixed(100.0));

    let value_text = format!("{value:.1}");

    container(
        column![slider, text(value_text).size(11)]
            .spacing(3)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(12.0))
    .into()
}

fn gain_slider(id: ParamId, value: f32) -> Element<'static, Message> {
    let def = PARAMS[id.as_index()];
    let s = slider(def.min as f32..=def.max as f32, value, move |v| {
        Message::SetParam(id, v)
    })
    .step(def.step as f32)
    .double_click_reset(def.default as f32)
    .width(Length::Fixed(20.0))
    .height(Length::Fixed(100.0));

    let value_text = format!("{value:.1} dB");

    container(
        column![s, text(value_text).size(10)]
            .spacing(2)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(40.0))
    .into()
}

fn build_app(shared: Arc<SharedState<ParamId>>) -> impl maolan_baseview::iced::Program {
    maolan_baseview::iced::application(move || init(shared.clone()), update, view)
        .theme(theme)
        .run()
}

pub struct GuiBridge {
    created: bool,
    floating: bool,
    shared: Option<Arc<SharedState<ParamId>>>,
    floating_open: Arc<AtomicBool>,
    window_handle: Option<AnyWindowHandle>,
}

impl Default for GuiBridge {
    fn default() -> Self {
        Self {
            created: false,
            floating: false,
            shared: None,
            floating_open: Arc::new(AtomicBool::new(false)),
            window_handle: None,
        }
    }
}

impl GuiBridge {
    pub fn create(
        &mut self,
        shared: Arc<SharedState<ParamId>>,
        api: &CStr,
        is_floating: bool,
    ) -> bool {
        if !is_api_supported(api, is_floating) {
            return false;
        }
        self.created = true;
        self.floating = is_floating;
        self.shared = Some(shared);
        true
    }

    pub fn destroy(&mut self) {
        self.window_handle = None;
        self.shared = None;
        self.floating = false;
        self.created = false;
    }

    pub fn set_parent(
        &mut self,
        shared: Arc<SharedState<ParamId>>,
        parent: ParentWindowHandle,
    ) -> bool {
        if !self.created {
            return false;
        }
        if self.floating {
            self.shared = Some(shared);
            return true;
        }

        let settings = maolan_baseview::iced::IcedBaseviewSettings {
            window: maolan_baseview::iced::baseview::WindowOpenOptions {
                title: String::from("Maolan Graphic EQ"),
                size: maolan_baseview::iced::baseview::Size::new(
                    EDITOR_WIDTH as f64,
                    EDITOR_HEIGHT as f64,
                ),
                scale: maolan_baseview::iced::baseview::WindowScalePolicy::SystemScaleFactor,
            },
            ignore_non_modifier_keys: false,
            always_redraw: false,
        };

        let handle = maolan_baseview::iced::shell::open_parented(
            &parent,
            settings,
            maolan_baseview::iced::PollSubNotifier::new(),
            move || build_app(shared),
        );

        self.window_handle = Some(AnyWindowHandle {
            _inner: Box::new(handle),
        });
        true
    }

    pub fn show(&mut self) -> bool {
        if !self.created {
            return false;
        }
        if self.floating {
            if self.floating_open.swap(true, Ordering::AcqRel) {
                return true;
            }
            let Some(shared) = self.shared.clone() else {
                self.floating_open.store(false, Ordering::Release);
                return false;
            };
            let open_flag = self.floating_open.clone();
            thread::spawn(move || {
                let settings = maolan_baseview::iced::IcedBaseviewSettings {
                    window: maolan_baseview::iced::baseview::WindowOpenOptions {
                        title: String::from("Maolan Graphic EQ"),
                        size: maolan_baseview::iced::baseview::Size::new(
                            EDITOR_WIDTH as f64,
                            EDITOR_HEIGHT as f64,
                        ),
                        scale:
                            maolan_baseview::iced::baseview::WindowScalePolicy::SystemScaleFactor,
                    },
                    ignore_non_modifier_keys: false,
                    always_redraw: false,
                };
                maolan_baseview::iced::shell::open_blocking(
                    settings,
                    maolan_baseview::iced::PollSubNotifier::new(),
                    move || build_app(shared),
                );
                open_flag.store(false, Ordering::Release);
            });
        }
        true
    }

    pub fn hide(&mut self, shared: Arc<SharedState<ParamId>>) -> bool {
        if self.floating {
            self.floating_open.store(false, Ordering::Release);
            shared.request_gui_closed();
            return true;
        }
        self.window_handle = None;
        true
    }
}
