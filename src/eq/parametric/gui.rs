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
use crate::eq::parametric::params::{PARAMS, ParamId};
use maolan_baseview::iced::{
    Alignment, Color, Element, Event, Length, Point, Rectangle, Renderer, Task, Theme,
    alignment::{Horizontal, Vertical},
    mouse,
    widget::{
        button, canvas,
        canvas::{Action as CanvasAction, Frame, Geometry, Path, Program, Text},
        checkbox, column, container, row, text,
    },
};
use maolan_widgets::arch_slider::arch_slider;
use maolan_widgets::meters::meters;
use maolan_widgets::slider::slider;

pub const EDITOR_WIDTH: u32 = 700;
pub const EDITOR_HEIGHT: u32 = 530;

#[derive(Debug, Clone)]
pub enum Message {
    SetParam(ParamId, f32),
    SetBandFreqGain(usize, f32, f32),
    EndBandDrag(usize),
    SetBoolParam(ParamId, bool),
    SelectTab(usize),
    ReleaseParam(ParamId),
    UiTick,
}

struct State {
    shared: Arc<SharedState<ParamId>>,
    selected_tab: usize,
    active_gestures: HashSet<ParamId>,
}

fn init(shared: Arc<SharedState<ParamId>>) -> (State, Task<Message>) {
    (
        State {
            shared,
            selected_tab: 0,
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
        Message::SetBandFreqGain(index, freq, gain) => {
            let freq = freq.clamp(20.0, 20_000.0);
            let gain = gain.clamp(-24.0, 24.0);
            let fid = ParamId::para_freq(index);
            let gid = ParamId::para_gain(index);
            if state.active_gestures.insert(fid) {
                state.shared.mark_gesture_begin_pending(fid);
            }
            if state.active_gestures.insert(gid) {
                state.shared.mark_gesture_begin_pending(gid);
            }
            state.shared.set_param_outbound_only(fid, freq as f64);
            state.shared.set_param_outbound_only(gid, gain as f64);
        }
        Message::EndBandDrag(index) => {
            let fid = ParamId::para_freq(index);
            let gid = ParamId::para_gain(index);
            if state.active_gestures.remove(&fid) {
                state.shared.mark_gesture_end_pending(fid);
            }
            if state.active_gestures.remove(&gid) {
                state.shared.mark_gesture_end_pending(gid);
            }
        }
        Message::SetBoolParam(id, value) => {
            if state.active_gestures.insert(id) {
                state.shared.mark_gesture_begin_pending(id);
            }
            state
                .shared
                .set_param_outbound_only(id, if value { 1.0 } else { 0.0 })
        }
        Message::SelectTab(tab) => state.selected_tab = tab.min(3),
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

    let mut tabs = row![].spacing(5).align_y(Alignment::Center);
    for tab in 0..4 {
        let label = format!("{}", tab + 1);
        let tab_element: Element<'_, Message> = if tab == state.selected_tab {
            container(text(label).size(14)).padding(5).into()
        } else {
            button(text(label).size(14))
                .on_press(Message::SelectTab(tab))
                .into()
        };
        tabs = tabs.push(tab_element);
    }

    let mut content = column![tabs].spacing(10).align_x(Alignment::Start);

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

    let start_band = state.selected_tab * 8;
    let mut band_points = Vec::with_capacity(8);
    for i in start_band..start_band + 8 {
        band_points.push((
            i,
            p(ParamId::para_freq(i)),
            p(ParamId::para_gain(i)),
            p(ParamId::para_q(i)),
            state.shared.params.get_bool(ParamId::para_on(i)),
        ));
    }
    let mut parametric_bands =
        row![container(meters(ch, &input_levels_db, 260.0)).height(Length::Fill)].spacing(10);
    for i in start_band..start_band + 8 {
        parametric_bands = parametric_bands.push(parametric_band(state, i));
    }
    parametric_bands =
        parametric_bands.push(container(meters(ch, &output_levels_db, 260.0)).height(Length::Fill));

    let parameters = column![parametric_bands]
        .spacing(10)
        .align_x(Alignment::Center);
    let output_spectrum_db = state.shared.output_spectrum_db();
    let response = eq_response_graph(band_points, output_spectrum_db);

    content = content.push(
        column![
            response,
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
        .align_x(Horizontal::Left)
        .align_y(Vertical::Top)
        .into()
}

#[derive(Default, Debug)]
struct EqResponseState {
    dragging: Option<usize>,
}

#[derive(Clone)]
struct EqResponseCanvas {
    bands: Vec<(usize, f32, f32, f32, bool)>, // (global index, freq, gain, q, on)
    output_spectrum_db: [f32; SPECTRUM_BINS],
}

impl EqResponseCanvas {
    const F_MIN: f32 = 20.0;
    const F_MAX: f32 = 20_000.0;
    const G_MIN: f32 = -24.0;
    const G_MAX: f32 = 24.0;
    const S_MIN: f32 = -90.0;
    const S_MAX: f32 = 0.0;

    fn freq_to_x(freq: f32, bounds: Rectangle) -> f32 {
        let f = freq.clamp(Self::F_MIN, Self::F_MAX);
        let t = (f / Self::F_MIN).ln() / (Self::F_MAX / Self::F_MIN).ln();
        bounds.x + t * bounds.width
    }

    fn x_to_freq(x: f32, bounds: Rectangle) -> f32 {
        let t = ((x - bounds.x) / bounds.width).clamp(0.0, 1.0);
        Self::F_MIN * (Self::F_MAX / Self::F_MIN).powf(t)
    }

    fn gain_to_y(gain: f32, bounds: Rectangle) -> f32 {
        let g = gain.clamp(Self::G_MIN, Self::G_MAX);
        let t = (g - Self::G_MIN) / (Self::G_MAX - Self::G_MIN);
        bounds.y + (1.0 - t) * bounds.height
    }

    fn y_to_gain(y: f32, bounds: Rectangle) -> f32 {
        let t = (1.0 - ((y - bounds.y) / bounds.height)).clamp(0.0, 1.0);
        Self::G_MIN + t * (Self::G_MAX - Self::G_MIN)
    }

    fn spectrum_to_y(db: f32, bounds: Rectangle) -> f32 {
        let s = db.clamp(Self::S_MIN, Self::S_MAX);
        let t = (s - Self::S_MIN) / (Self::S_MAX - Self::S_MIN);
        bounds.y + (1.0 - t) * bounds.height
    }
}

impl Program<Message> for EqResponseCanvas {
    type State = EqResponseState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<CanvasAction<Message>> {
        let local_bounds = Rectangle {
            x: 0.0,
            y: 0.0,
            width: bounds.width,
            height: bounds.height,
        };
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(p) = cursor.position_in(bounds) {
                    let mut closest = None;
                    let mut best_d2 = 12.0_f32 * 12.0_f32;
                    for (local_idx, (_global_idx, freq, gain, _q, on)) in
                        self.bands.iter().enumerate()
                    {
                        if !*on {
                            continue;
                        }
                        let x = Self::freq_to_x(*freq, local_bounds);
                        let y = Self::gain_to_y(*gain, local_bounds);
                        let dx = p.x - x;
                        let dy = p.y - y;
                        let d2 = dx * dx + dy * dy;
                        if d2 <= best_d2 {
                            best_d2 = d2;
                            closest = Some(local_idx);
                        }
                    }
                    state.dragging = closest;
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if let Some(local_idx) = state.dragging.take()
                    && let Some((global_idx, _freq, _gain, _q, _on)) =
                        self.bands.get(local_idx).copied()
                {
                    return Some(CanvasAction::publish(Message::EndBandDrag(global_idx)));
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let (Some(local_idx), Some(p)) = (state.dragging, cursor.position_in(bounds))
                    && let Some((global_idx, _freq, _gain, _q, _on)) =
                        self.bands.get(local_idx).copied()
                {
                    let freq = Self::x_to_freq(p.x, local_bounds);
                    let gain = Self::y_to_gain(p.y, local_bounds);
                    return Some(
                        CanvasAction::publish(Message::SetBandFreqGain(global_idx, freq, gain))
                            .and_capture(),
                    );
                }
            }
            _ => {}
        }
        None
    }

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

        let h_grid_db = [-24.0_f32, -18.0, -12.0, -6.0, 0.0, 6.0, 12.0, 18.0, 24.0];
        for db in h_grid_db {
            let y = Self::gain_to_y(
                db,
                Rectangle {
                    x: 0.0,
                    y: 0.0,
                    ..bounds
                },
            );
            let path = Path::line(Point::new(0.0, y), Point::new(bounds.width, y));
            let c = if db == 0.0 {
                Color::from_rgba(0.85, 0.87, 0.90, 0.28)
            } else {
                Color::from_rgba(0.72, 0.76, 0.82, 0.12)
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

        let response = Path::new(|b| {
            let mut first = true;
            for xi in 0..(bounds.width as usize).max(2) {
                let x = xi as f32;
                let freq = Self::x_to_freq(
                    x,
                    Rectangle {
                        x: 0.0,
                        y: 0.0,
                        ..bounds
                    },
                );
                let mut total_db = 0.0_f32;
                for (_idx, f0, gain_db, q, on) in &self.bands {
                    if *on {
                        total_db += bell_like_db(freq, *f0, *gain_db, *q);
                    }
                }
                let y = Self::gain_to_y(
                    total_db,
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
            &response,
            canvas::Stroke::default()
                .with_color(Color::from_rgb(0.53, 0.88, 0.98))
                .with_width(2.0),
        );

        let spectrum = Path::new(|b| {
            let mut first = true;
            for (i, db) in self.output_spectrum_db.iter().enumerate() {
                let t = i as f32 / (SPECTRUM_BINS.saturating_sub(1).max(1) as f32);
                let x = t * bounds.width;
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
                .with_color(Color::from_rgba(0.95, 0.95, 0.95, 0.75))
                .with_width(1.0),
        );

        for (_global_idx, freq, gain, _q, on) in self.bands.iter() {
            if !*on {
                continue;
            }
            let x = Self::freq_to_x(
                *freq,
                Rectangle {
                    x: 0.0,
                    y: 0.0,
                    ..bounds
                },
            );
            let y = Self::gain_to_y(
                *gain,
                Rectangle {
                    x: 0.0,
                    y: 0.0,
                    ..bounds
                },
            );
            let node = Path::circle(Point::new(x, y), 4.5);
            frame.fill(&node, Color::from_rgb(0.95, 0.64, 0.18));
            frame.stroke(
                &node,
                canvas::Stroke::default()
                    .with_color(Color::from_rgb(0.16, 0.16, 0.18))
                    .with_width(1.0),
            );

            let label = format_freq(*freq);
            let label_x = (x - 22.0).clamp(0.0, (bounds.width - 48.0).max(0.0));
            let label_y = (y - 12.0).max(10.0);
            frame.fill_text(Text {
                content: label,
                position: Point::new(label_x, label_y),
                color: Color::from_rgb(0.95, 0.95, 0.98),
                size: 10.0.into(),
                ..Text::default()
            });
        }

        vec![frame.into_geometry()]
    }
}

fn bell_like_db(freq: f32, f0: f32, gain_db: f32, q: f32) -> f32 {
    let safe_f0 = f0.clamp(20.0, 20_000.0);
    let safe_q = q.clamp(0.1, 24.0);
    let dx = (freq / safe_f0).ln();
    let sigma = 1.0 / safe_q;
    gain_db * (-0.5 * (dx / sigma).powi(2)).exp()
}

fn eq_response_graph(
    bands: Vec<(usize, f32, f32, f32, bool)>,
    output_spectrum_db: [f32; SPECTRUM_BINS],
) -> Element<'static, Message> {
    canvas(EqResponseCanvas {
        bands,
        output_spectrum_db,
    })
    .width(Length::Fill)
    .height(Length::Fixed(190.0))
    .into()
}

fn parametric_band(state: &State, index: usize) -> Element<'_, Message> {
    let p = |id: ParamId| state.shared.params.get(id) as f32;
    let fid = ParamId::para_freq(index);
    let gid = ParamId::para_gain(index);
    let qid = ParamId::para_q(index);
    let oid = ParamId::para_on(index);
    let label = format!("P{:02}", index + 1);
    let on = state.shared.params.get_bool(oid);

    column![
        text(label).size(14),
        checkbox(on)
            .label("On")
            .on_toggle(move |v| Message::SetBoolParam(oid, v)),
        freq_knob(fid, p(fid)),
        knob("Gain".to_string(), gid, p(gid), "dB", 0.1),
        knob("Q".to_string(), qid, p(qid), "", 0.01),
    ]
    .spacing(5)
    .align_x(Alignment::Center)
    .into()
}

fn theme(_state: &State) -> Theme {
    Theme::TokyoNight
}

fn knob(
    label: String,
    id: ParamId,
    value: f32,
    units: &'static str,
    step: f32,
) -> Element<'static, Message> {
    let def = PARAMS[id.as_index()];
    let slider = arch_slider(def.min as f32..=def.max as f32, value, move |v| {
        Message::SetParam(id, v)
    })
    .step(step)
    .double_click_reset(def.default as f32)
    .on_release(Message::ReleaseParam(id))
    .fill_from_start()
    .width(Length::Fixed(41.0))
    .height(Length::Fixed(41.0));

    let value_text = if units.is_empty() {
        format!("{value:.2}")
    } else if units == "Hz" {
        format!("{value:.0} {units}")
    } else {
        format!("{value:.1} {units}")
    };

    container(
        column![text(label).size(11), slider, text(value_text).size(10)]
            .spacing(2)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(50.0))
    .into()
}

fn freq_to_norm(freq_hz: f32) -> f32 {
    let f_min = 20.0_f32;
    let f_mid = 1000.0_f32;
    let f_max = 20_000.0_f32;
    let f = freq_hz.max(f_min).min(f_max);
    if f <= f_mid {
        0.5 * ((f / f_min).ln() / (f_mid / f_min).ln())
    } else {
        0.5 + 0.5 * ((f / f_mid).ln() / (f_max / f_mid).ln())
    }
}

fn norm_to_freq(norm: f32) -> f32 {
    let f_min = 20.0_f32;
    let f_mid = 1000.0_f32;
    let f_max = 20_000.0_f32;
    let t = norm.clamp(0.0, 1.0);
    if t <= 0.5 {
        f_min * (f_mid / f_min).powf(t / 0.5)
    } else {
        f_mid * (f_max / f_mid).powf((t - 0.5) / 0.5)
    }
}

fn format_freq(freq_hz: f32) -> String {
    if freq_hz >= 1000.0 {
        format!("{:.2}k", freq_hz / 1000.0)
    } else {
        format!("{freq_hz:.0}")
    }
}

fn freq_knob(id: ParamId, value_hz: f32) -> Element<'static, Message> {
    let def = PARAMS[id.as_index()];
    let value_norm = freq_to_norm(value_hz);
    let default_norm = freq_to_norm(def.default as f32);
    let slider = arch_slider(0.0_f32..=1.0_f32, value_norm, move |n| {
        Message::SetParam(id, norm_to_freq(n))
    })
    .double_click_reset(default_norm)
    .on_release(Message::ReleaseParam(id))
    .fill_from_start()
    .width(Length::Fixed(41.0))
    .height(Length::Fixed(41.0));

    container(
        column![text("Freq").size(11), slider, text("").size(10)]
            .spacing(2)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(50.0))
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
    .height(Length::Fill);

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
                title: String::from("Maolan Parametric EQ"),
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
                        title: String::from("Maolan Parametric EQ"),
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
