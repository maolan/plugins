use std::{
    collections::HashSet,
    ffi::CStr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use crate::eq::common::gui::{AnyWindowHandle, ParentWindowHandle, is_api_supported};
use crate::eq::common::params::ParamIdExt;
use crate::eq::common::plugin::{SPECTRUM_BINS, SharedState};
use crate::eq::graphic::params::{PARAMS, ParamId};
use maolan_baseview::iced::{
    Alignment, Background, Color, Element, Length, Point, Rectangle, Renderer, Task, Theme,
    alignment::{Horizontal, Vertical},
    mouse,
    widget::{
        canvas,
        canvas::{Frame, Geometry, Path, Program},
        column, container, row, scrollable,
        scrollable::Scrollbar,
        text,
    },
};
use maolan_widgets::meters::meters;
use maolan_widgets::slider::slider;

pub const EDITOR_WIDTH: u32 = 900;
pub const EDITOR_HEIGHT: u32 = 380;

#[derive(Debug, Clone)]
pub enum Message {
    SetParam(ParamId, f32),
    SetBoolParam(ParamId, bool),
    ReleaseParam(ParamId),
    UiTick,
}

struct State {
    shared: Arc<SharedState<ParamId>>,
    active_gestures: HashSet<ParamId>,
}

fn init(shared: Arc<SharedState<ParamId>>) -> (State, Task<Message>) {
    (
        State {
            shared,
            active_gestures: HashSet::new(),
        },
        next_ui_tick_task(),
    )
}

fn next_ui_tick_task() -> Task<Message> {
    Task::perform(
        async move {
            thread::sleep(Duration::from_millis(33));
        },
        |_| Message::UiTick,
    )
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::SetParam(id, value) => {
            if state.active_gestures.insert(id) {
                state.shared.mark_gesture_begin_pending(id);
            }
            state.shared.set_param_outbound_only(id, value as f64);
        }
        Message::SetBoolParam(id, value) => {
            if state.active_gestures.insert(id) {
                state.shared.mark_gesture_begin_pending(id);
            }
            state
                .shared
                .set_param_outbound_only(id, if value { 1.0 } else { 0.0 })
        }
        Message::ReleaseParam(id) => {
            if state.active_gestures.remove(&id) {
                state.shared.mark_gesture_end_pending(id);
            }
        }
        Message::UiTick => return next_ui_tick_task(),
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
            .direction(scrollable::Direction::Horizontal(Scrollbar::hidden()))
            .height(Length::Fixed(150.0)),
    );
    graphic_row =
        graphic_row.push(container(meters(ch, &output_levels_db, 150.0)).height(Length::Fill));

    let parameters = column![graphic_row].spacing(10).align_x(Alignment::Center);
    let output_spectrum_db = state.shared.output_spectrum_db();

    content = content.push(
        column![
            output_spectrum_graph(output_spectrum_db),
            row![
                gain_slider(ParamId::InputGain, p(ParamId::InputGain)),
                parameters,
                gain_slider(ParamId::OutputGain, p(ParamId::OutputGain)),
            ]
            .spacing(14)
            .align_y(Alignment::Start),
        ]
        .spacing(10),
    );

    container(content)
        .padding(16)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_theme| maolan_baseview::iced::widget::container::Style {
            background: Some(Background::Color(Color::from_rgb(0.10, 0.11, 0.14))),
            ..Default::default()
        })
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
    .on_release(Message::ReleaseParam(id))
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
    .on_release(Message::ReleaseParam(id))
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

#[derive(Clone)]
struct SpectrumCanvas {
    output_spectrum_db: [f32; SPECTRUM_BINS],
}

impl SpectrumCanvas {
    const F_MIN: f32 = 20.0;
    const F_MAX: f32 = 20_000.0;
    const S_MIN: f32 = -90.0;
    const S_MAX: f32 = 0.0;

    fn freq_to_x(freq: f32, bounds: Rectangle) -> f32 {
        let f = freq.clamp(Self::F_MIN, Self::F_MAX);
        let t = (f / Self::F_MIN).ln() / (Self::F_MAX / Self::F_MIN).ln();
        bounds.x + t * bounds.width
    }

    fn spectrum_to_y(db: f32, bounds: Rectangle) -> f32 {
        let s = db.clamp(Self::S_MIN, Self::S_MAX);
        let t = (s - Self::S_MIN) / (Self::S_MAX - Self::S_MIN);
        bounds.y + (1.0 - t) * bounds.height
    }
}

impl Program<Message> for SpectrumCanvas {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        frame.fill(
            &Path::rectangle(Point::new(0.0, 0.0), bounds.size()),
            Color::from_rgb(0.10, 0.11, 0.14),
        );

        let h_grid_db = [-90.0_f32, -72.0, -54.0, -36.0, -18.0, 0.0];
        for db in h_grid_db {
            let y = Self::spectrum_to_y(
                db,
                Rectangle {
                    x: 0.0,
                    y: 0.0,
                    ..bounds
                },
            );
            let path = Path::line(Point::new(0.0, y), Point::new(bounds.width, y));
            let c = if db == -18.0 {
                Color::from_rgba(0.85, 0.87, 0.90, 0.24)
            } else {
                Color::from_rgba(0.72, 0.76, 0.82, 0.10)
            };
            frame.stroke(
                &path,
                canvas::Stroke::default().with_color(c).with_width(1.0),
            );
        }

        let v_grid_hz = [
            20.0_f32, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0, 20_000.0,
        ];
        for hz in v_grid_hz {
            let x = Self::freq_to_x(
                hz,
                Rectangle {
                    x: 0.0,
                    y: 0.0,
                    ..bounds
                },
            );
            let path = Path::line(Point::new(x, 0.0), Point::new(x, bounds.height));
            frame.stroke(
                &path,
                canvas::Stroke::default()
                    .with_color(Color::from_rgba(0.72, 0.76, 0.82, 0.10))
                    .with_width(1.0),
            );
        }

        let spectrum = Path::new(|b| {
            let mut first = true;
            for (i, db) in self.output_spectrum_db.iter().enumerate() {
                let t = i as f32 / (SPECTRUM_BINS.saturating_sub(1).max(1) as f32);
                let freq = Self::F_MIN * (Self::F_MAX / Self::F_MIN).powf(t);
                let x = Self::freq_to_x(
                    freq,
                    Rectangle {
                        x: 0.0,
                        y: 0.0,
                        ..bounds
                    },
                );
                let y = Self::spectrum_to_y(
                    *db,
                    Rectangle {
                        x: 0.0,
                        y: 0.0,
                        ..bounds
                    },
                );
                if first {
                    b.move_to(Point::new(x, y));
                    first = false;
                } else {
                    b.line_to(Point::new(x, y));
                }
            }
        });
        frame.stroke(
            &spectrum,
            canvas::Stroke::default()
                .with_color(Color::from_rgba(0.95, 0.95, 0.95, 0.85))
                .with_width(1.2),
        );

        vec![frame.into_geometry()]
    }
}

fn output_spectrum_graph(output_spectrum_db: [f32; SPECTRUM_BINS]) -> Element<'static, Message> {
    canvas(SpectrumCanvas { output_spectrum_db })
        .width(Length::Fill)
        .height(Length::Fixed(190.0))
        .into()
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
        if let Some(shared) = &self.shared {
            shared.set_ui_visible(false);
        }
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
        shared.set_ui_visible(true);

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
            always_redraw: true,
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
            shared.set_ui_visible(true);
            let open_flag = self.floating_open.clone();
            thread::spawn(move || {
                let shared_for_close = shared.clone();
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
                    always_redraw: true,
                };
                maolan_baseview::iced::shell::open_blocking(
                    settings,
                    maolan_baseview::iced::PollSubNotifier::new(),
                    move || build_app(shared),
                );
                open_flag.store(false, Ordering::Release);
                shared_for_close.set_ui_visible(false);
            });
        }
        true
    }

    pub fn hide(&mut self, shared: Arc<SharedState<ParamId>>) -> bool {
        shared.set_ui_visible(false);
        if self.floating {
            self.floating_open.store(false, Ordering::Release);
            shared.request_gui_closed();
            return true;
        }
        self.window_handle = None;
        true
    }
}
