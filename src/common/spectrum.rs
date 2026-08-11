use crate::common::fft::SpectrumAnalyzer;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use maolan_baseview::iced::{
    Color, Element, Event, Length, Point, Rectangle, Renderer, Theme, mouse,
    widget::{
        canvas,
        canvas::{
            self as canvas_module, Action as CanvasAction, Frame, Geometry, Path, Program, Text,
        },
    },
};

pub const FFT_SIZE: usize = 4096;
pub const SPECTRUM_FLOOR_DB: f32 = -90.0;
pub const DEFAULT_SPECTRUM_BINS: usize = 192;

/// Real-time spectrum analyzer feeding the EQ display: single-channel ring
/// buffer, Hann-windowed 4096-point FFT, power remap onto log-spaced display
/// bins, and peak-hold/decay smoothing per bin.
pub struct LogSpectrumAnalyzer {
    fft: SpectrumAnalyzer,
    ring: Vec<f32>,
    write_pos: usize,
    hann: Vec<f32>,
    windowed: Vec<f32>,
    mags: Vec<f32>,
    powers: Vec<f32>,
    display_power: Vec<f32>,
    smoothed_db: Vec<f32>,
    /// Geometric edges of the display bins, as FFT bin indices at 48 kHz
    /// reference, recomputed per `compute` call for the actual sample rate.
    bins: usize,
}

pub struct SpectrumMarker<Message> {
    pub freq_hz: f32,
    pub on_drag: Option<Arc<dyn Fn(f32) -> Message + Send + Sync>>,
    pub on_release: Option<Message>,
}

impl<Message: Clone> Clone for SpectrumMarker<Message> {
    fn clone(&self) -> Self {
        Self {
            freq_hz: self.freq_hz,
            on_drag: self.on_drag.clone(),
            on_release: self.on_release.clone(),
        }
    }
}

impl<Message> SpectrumMarker<Message> {
    pub fn passive(freq_hz: f32) -> Self {
        Self {
            freq_hz,
            on_drag: None,
            on_release: None,
        }
    }

    pub fn draggable(
        freq_hz: f32,
        on_drag: impl Fn(f32) -> Message + Send + Sync + 'static,
        on_release: Message,
    ) -> Self {
        Self {
            freq_hz,
            on_drag: Some(Arc::new(on_drag)),
            on_release: Some(on_release),
        }
    }
}

pub struct SpectrumThresholdCurve<Message> {
    pub points: Vec<(f32, f32)>,
    pub selected: bool,
    pub color: Option<Color>,
    pub selected_color: Option<Color>,
    pub width: f32,
    pub selected_width: f32,
    pub on_select: Option<Message>,
    pub on_select_at: Option<Arc<dyn Fn(f32) -> Message + Send + Sync>>,
    pub on_drag_db: Option<Arc<dyn Fn(f32) -> Message + Send + Sync>>,
    pub on_drag_db_at: Option<Arc<dyn Fn(f32, f32) -> Message + Send + Sync>>,
    pub on_double_click_at: Option<Arc<dyn Fn(f32) -> Message + Send + Sync>>,
    pub on_release: Option<Message>,
    pub on_release_at: Option<Arc<dyn Fn(f32) -> Message + Send + Sync>>,
    pub on_middle_select_at: Option<Arc<dyn Fn(f32) -> Message + Send + Sync>>,
    pub on_middle_drag_db_at: Option<Arc<dyn Fn(f32, f32) -> Message + Send + Sync>>,
    pub on_middle_release_at: Option<Arc<dyn Fn(f32) -> Message + Send + Sync>>,
}

impl<Message: Clone> Clone for SpectrumThresholdCurve<Message> {
    fn clone(&self) -> Self {
        Self {
            points: self.points.clone(),
            selected: self.selected,
            color: self.color,
            selected_color: self.selected_color,
            width: self.width,
            selected_width: self.selected_width,
            on_select: self.on_select.clone(),
            on_select_at: self.on_select_at.clone(),
            on_drag_db: self.on_drag_db.clone(),
            on_drag_db_at: self.on_drag_db_at.clone(),
            on_double_click_at: self.on_double_click_at.clone(),
            on_release: self.on_release.clone(),
            on_release_at: self.on_release_at.clone(),
            on_middle_select_at: self.on_middle_select_at.clone(),
            on_middle_drag_db_at: self.on_middle_drag_db_at.clone(),
            on_middle_release_at: self.on_middle_release_at.clone(),
        }
    }
}

#[derive(Clone)]
pub struct SpectralAnalyzerWidget<Message, const BINS: usize> {
    pub bins_db: [[f32; BINS]; 2],
    pub stereo: bool,
    pub display_range_db: f32,
    pub markers: Vec<SpectrumMarker<Message>>,
    pub threshold_curves: Vec<SpectrumThresholdCurve<Message>>,
    pub gain_reduction_db: Vec<f32>,
    pub on_double_click: Option<Arc<dyn Fn(f32, f32) -> Message + Send + Sync>>,
}

impl<Message: Clone + 'static, const BINS: usize> SpectralAnalyzerWidget<Message, BINS> {
    const F_MIN: f32 = 20.0;
    const F_MAX: f32 = 20_000.0;
    const DEFAULT_DISPLAY_RANGE_DB: f32 = 0.0;

    pub fn new(bins_db: [[f32; BINS]; 2], stereo: bool) -> Self {
        Self {
            bins_db,
            stereo,
            display_range_db: Self::DEFAULT_DISPLAY_RANGE_DB,
            markers: Vec::new(),
            threshold_curves: Vec::new(),
            gain_reduction_db: Vec::new(),
            on_double_click: None,
        }
    }

    pub fn with_markers(mut self, markers_hz: Vec<f32>) -> Self {
        self.markers = markers_hz
            .into_iter()
            .map(SpectrumMarker::passive)
            .collect();
        self
    }

    pub fn with_display_range(mut self, range_db: f32) -> Self {
        self.display_range_db = range_db.clamp(1.5, 90.0);
        self
    }

    pub fn with_spectrum_markers(mut self, markers: Vec<SpectrumMarker<Message>>) -> Self {
        self.markers = markers;
        self
    }

    pub fn with_threshold_curves(mut self, curves: Vec<SpectrumThresholdCurve<Message>>) -> Self {
        self.threshold_curves = curves;
        self
    }

    pub fn with_gain_reduction(mut self, gain_reduction_db: Vec<f32>) -> Self {
        self.gain_reduction_db = gain_reduction_db;
        self
    }

    pub fn on_double_click(
        mut self,
        on_double_click: impl Fn(f32, f32) -> Message + Send + Sync + 'static,
    ) -> Self {
        self.on_double_click = Some(Arc::new(on_double_click));
        self
    }

    pub fn view(self) -> Element<'static, Message> {
        self.view_with_height(Length::Fixed(300.0))
    }

    pub fn view_fill(self) -> Element<'static, Message> {
        self.view_with_height(Length::Fill)
    }

    pub fn view_with_height(self, height: Length) -> Element<'static, Message> {
        canvas(self).width(Length::Fill).height(height).into()
    }

    fn freq_to_x(freq: f32, bounds: Rectangle) -> f32 {
        let f = freq.clamp(Self::F_MIN, Self::F_MAX);
        let t = (f / Self::F_MIN).ln() / (Self::F_MAX / Self::F_MIN).ln();
        bounds.x + t * bounds.width
    }

    fn x_to_freq(x: f32, bounds: Rectangle) -> f32 {
        let t = ((x - bounds.x) / bounds.width).clamp(0.0, 1.0);
        Self::F_MIN * (Self::F_MAX / Self::F_MIN).powf(t)
    }

    fn min_db(&self) -> f32 {
        if self.display_range_db > 0.0 {
            -self.display_range_db.clamp(1.5, 30.0) * 1.75
        } else {
            -60.0
        }
    }

    fn max_db(&self) -> f32 {
        if self.display_range_db > 0.0 {
            self.display_range_db.clamp(1.5, 30.0)
        } else {
            0.0
        }
    }

    fn spectrum_to_y(&self, db: f32, bounds: Rectangle) -> f32 {
        let min = self.min_db();
        let max = self.max_db();
        let db = db.clamp(min, max);
        let t = (db - min) / (max - min);
        bounds.y + (1.0 - t) * bounds.height
    }

    fn y_to_spectrum_db(&self, y: f32, bounds: Rectangle) -> f32 {
        let min = self.min_db();
        let max = self.max_db();
        let t = (1.0 - ((y - bounds.y) / bounds.height)).clamp(0.0, 1.0);
        min + t * (max - min)
    }

    fn smoothed_points(&self, bins_db: &[f32; BINS], bounds: Rectangle) -> Vec<Point> {
        bins_db
            .iter()
            .enumerate()
            .map(|(i, &db)| {
                let t = i as f32 / (BINS.saturating_sub(1).max(1) as f32);
                Point::new(t * bounds.width, self.spectrum_to_y(db, bounds))
            })
            .collect()
    }

    fn draw_smooth_points(points: &[Point], b: &mut canvas_module::path::Builder) {
        let Some(&first) = points.first() else {
            return;
        };
        b.move_to(first);
        if points.len() == 1 {
            return;
        }
        if points.len() == 2 {
            b.line_to(points[1]);
            return;
        }

        for i in 0..points.len() - 1 {
            let p0 = if i == 0 { points[i] } else { points[i - 1] };
            let p1 = points[i];
            let p2 = points[i + 1];
            let p3 = if i + 2 < points.len() {
                points[i + 2]
            } else {
                points[i + 1]
            };
            let c1 = Point::new(p1.x + (p2.x - p0.x) / 6.0, p1.y + (p2.y - p0.y) / 6.0);
            let c2 = Point::new(p2.x - (p3.x - p1.x) / 6.0, p2.y - (p3.y - p1.y) / 6.0);
            b.bezier_curve_to(c1, c2, p2);
        }
    }

    fn spectrum_path(&self, bins_db: &[f32; BINS], bounds: Rectangle) -> Path {
        let points = self.smoothed_points(bins_db, bounds);
        Path::new(|b| {
            Self::draw_smooth_points(&points, b);
        })
    }

    fn spectrum_fill_path(&self, bins_db: &[f32; BINS], bounds: Rectangle) -> Path {
        let points = self.smoothed_points(bins_db, bounds);
        Path::new(|b| {
            Self::draw_smooth_points(&points, b);
            b.line_to(Point::new(bounds.width, bounds.height));
            b.line_to(Point::new(0.0, bounds.height));
            b.close();
        })
    }

    fn threshold_curve_points(
        &self,
        curve: &SpectrumThresholdCurve<Message>,
        bounds: Rectangle,
    ) -> Vec<Point> {
        curve
            .points
            .iter()
            .map(|&(freq, db)| {
                Point::new(
                    Self::freq_to_x(freq, bounds),
                    self.spectrum_to_y(db, bounds),
                )
            })
            .collect()
    }

    fn threshold_curve_path(
        &self,
        curve: &SpectrumThresholdCurve<Message>,
        bounds: Rectangle,
    ) -> Path {
        let points = self.threshold_curve_points(curve, bounds);
        Path::new(|b| {
            Self::draw_smooth_points(&points, b);
        })
    }

    fn closest_curve(&self, pos: Point, bounds: Rectangle) -> Option<usize> {
        let mut closest = None;
        let mut closest_distance = 8.0_f32;
        for (index, curve) in self.threshold_curves.iter().enumerate() {
            if curve.on_drag_db.is_none()
                && curve.on_drag_db_at.is_none()
                && curve.on_select.is_none()
                && curve.on_select_at.is_none()
                && curve.on_double_click_at.is_none()
                && curve.on_middle_select_at.is_none()
                && curve.on_middle_drag_db_at.is_none()
            {
                continue;
            }
            let distance = Self::curve_distance(pos, &self.threshold_curve_points(curve, bounds));
            if distance < closest_distance {
                closest = Some(index);
                closest_distance = distance;
            }
        }
        closest
    }

    fn curve_distance(pos: Point, points: &[Point]) -> f32 {
        points
            .windows(2)
            .map(|segment| Self::point_segment_distance(pos, segment[0], segment[1]))
            .fold(f32::INFINITY, f32::min)
    }

    fn point_segment_distance(pos: Point, a: Point, b: Point) -> f32 {
        let ab = Point::new(b.x - a.x, b.y - a.y);
        let ap = Point::new(pos.x - a.x, pos.y - a.y);
        let len_sq = ab.x * ab.x + ab.y * ab.y;
        if len_sq <= f32::EPSILON {
            return ((pos.x - a.x).powi(2) + (pos.y - a.y).powi(2)).sqrt();
        }
        let t = ((ap.x * ab.x + ap.y * ab.y) / len_sq).clamp(0.0, 1.0);
        let closest = Point::new(a.x + ab.x * t, a.y + ab.y * t);
        ((pos.x - closest.x).powi(2) + (pos.y - closest.y).powi(2)).sqrt()
    }
}

#[derive(Default)]
pub struct SpectralAnalyzerState {
    dragging_marker: Option<usize>,
    dragging_curve: Option<(usize, f32, f32, mouse::Button)>,
    last_click: Option<(Instant, Point)>,
}

impl<Message: Clone + 'static, const BINS: usize> Program<Message>
    for SpectralAnalyzerWidget<Message, BINS>
{
    type State = SpectralAnalyzerState;

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
                let pos = cursor.position_in(bounds)?;
                let now = Instant::now();
                let mut closest = None;
                let mut closest_dx = 8.0_f32;
                for (index, marker) in self.markers.iter().enumerate() {
                    if marker.on_drag.is_none() {
                        continue;
                    }
                    let x = Self::freq_to_x(marker.freq_hz, local_bounds);
                    let dx = (pos.x - x).abs();
                    if dx < closest_dx {
                        closest = Some(index);
                        closest_dx = dx;
                    }
                }
                state.dragging_marker = closest;
                if closest.is_some() {
                    state.last_click = None;
                    return Some(CanvasAction::capture());
                }

                let is_double = state
                    .last_click
                    .take()
                    .map(|(last_time, last_pos)| {
                        now.duration_since(last_time) <= Duration::from_millis(400)
                            && ((pos.x - last_pos.x).powi(2) + (pos.y - last_pos.y).powi(2)).sqrt()
                                <= 6.0
                    })
                    .unwrap_or(false);

                let freq = Self::x_to_freq(pos.x, local_bounds);
                if let Some(curve_index) = self.closest_curve(pos, local_bounds) {
                    if is_double
                        && let Some(message) = self
                            .threshold_curves
                            .get(curve_index)
                            .and_then(|curve| curve.on_double_click_at.as_ref())
                            .map(|on_double_click| on_double_click(freq))
                    {
                        state.dragging_curve = None;
                        state.last_click = None;
                        return Some(CanvasAction::publish(message).and_capture());
                    }
                    state.last_click = Some((now, pos));
                    state.dragging_curve = Some((curve_index, pos.y, freq, mouse::Button::Left));
                    let action = self
                        .threshold_curves
                        .get(curve_index)
                        .and_then(|curve| {
                            curve
                                .on_select_at
                                .as_ref()
                                .map(|on_select| on_select(freq))
                                .or_else(|| curve.on_select.clone())
                        })
                        .map(CanvasAction::publish)
                        .unwrap_or_else(CanvasAction::capture)
                        .and_capture();
                    return Some(action);
                }

                if is_double && let Some(on_double_click) = &self.on_double_click {
                    return Some(
                        CanvasAction::publish(on_double_click(
                            freq,
                            self.y_to_spectrum_db(pos.y, local_bounds),
                        ))
                        .and_capture(),
                    );
                }
                state.last_click = Some((now, pos));
                None
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Middle)) => {
                let pos = cursor.position_in(bounds)?;
                if let Some(curve_index) = self.closest_curve(pos, local_bounds) {
                    state.last_click = None;
                    let freq = Self::x_to_freq(pos.x, local_bounds);
                    state.dragging_curve = Some((curve_index, pos.y, freq, mouse::Button::Middle));
                    let action = self
                        .threshold_curves
                        .get(curve_index)
                        .and_then(|curve| {
                            curve
                                .on_middle_select_at
                                .as_ref()
                                .map(|on_select| on_select(freq))
                                .or_else(|| {
                                    curve.on_select_at.as_ref().map(|on_select| on_select(freq))
                                })
                                .or_else(|| curve.on_select.clone())
                        })
                        .map(CanvasAction::publish)
                        .unwrap_or_else(CanvasAction::capture)
                        .and_capture();
                    return Some(action);
                }
                None
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let pos = cursor.position_in(bounds)?;
                if let Some(marker_index) = state.dragging_marker {
                    let marker = self.markers.get(marker_index)?;
                    let on_drag = marker.on_drag.as_ref()?;
                    return Some(
                        CanvasAction::publish(on_drag(Self::x_to_freq(pos.x, local_bounds)))
                            .and_capture(),
                    );
                }

                if let Some((curve_index, last_y, drag_freq, button)) = state.dragging_curve {
                    let curve = self.threshold_curves.get(curve_index)?;
                    let last_db = self.y_to_spectrum_db(last_y, local_bounds);
                    let next_db = self.y_to_spectrum_db(pos.y, local_bounds);
                    state.last_click = None;
                    state.dragging_curve = Some((curve_index, pos.y, drag_freq, button));
                    let message = if button == mouse::Button::Middle {
                        curve
                            .on_middle_drag_db_at
                            .as_ref()
                            .map(|on_drag| on_drag(drag_freq, next_db - last_db))
                            .or_else(|| {
                                curve
                                    .on_drag_db_at
                                    .as_ref()
                                    .map(|on_drag| on_drag(drag_freq, next_db - last_db))
                            })
                    } else {
                        curve
                            .on_drag_db_at
                            .as_ref()
                            .map(|on_drag| on_drag(drag_freq, next_db - last_db))
                            .or_else(|| {
                                curve
                                    .on_drag_db
                                    .as_ref()
                                    .map(|on_drag| on_drag(next_db - last_db))
                            })
                    }?;
                    return Some(CanvasAction::publish(message).and_capture());
                }

                None
            }
            Event::Mouse(mouse::Event::ButtonReleased(
                mouse::Button::Left | mouse::Button::Middle,
            )) => {
                if let Some(marker_index) = state.dragging_marker.take() {
                    return self
                        .markers
                        .get(marker_index)
                        .and_then(|marker| marker.on_release.clone())
                        .map(|message| CanvasAction::publish(message).and_capture())
                        .or_else(|| Some(CanvasAction::capture()));
                }

                if let Some((curve_index, _, drag_freq, button)) = state.dragging_curve.take() {
                    return self
                        .threshold_curves
                        .get(curve_index)
                        .and_then(|curve| {
                            if button == mouse::Button::Middle {
                                curve
                                    .on_middle_release_at
                                    .as_ref()
                                    .map(|on_release| on_release(drag_freq))
                                    .or_else(|| {
                                        curve
                                            .on_release_at
                                            .as_ref()
                                            .map(|on_release| on_release(drag_freq))
                                    })
                                    .or_else(|| curve.on_release.clone())
                            } else {
                                curve
                                    .on_release_at
                                    .as_ref()
                                    .map(|on_release| on_release(drag_freq))
                                    .or_else(|| curve.on_release.clone())
                            }
                        })
                        .map(|message| CanvasAction::publish(message).and_capture())
                        .or_else(|| Some(CanvasAction::capture()));
                }

                None
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: maolan_baseview::iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let local_bounds = Rectangle {
            x: 0.0,
            y: 0.0,
            width: bounds.width,
            height: bounds.height,
        };
        let mut frame = Frame::new(renderer, bounds.size());
        frame.fill(
            &Path::rectangle(Point::new(0.0, 0.0), bounds.size()),
            Color::from_rgb(0.098, 0.098, 0.106),
        );

        let min = self.min_db();
        let max = self.max_db();
        for db in [min, min * 0.5, 0.0, max * 0.5, max] {
            let y = self.spectrum_to_y(db, local_bounds);
            let path = Path::line(Point::new(0.0, y), Point::new(bounds.width, y));
            let c = if db == 0.0 {
                Color::from_rgba(0.85, 0.87, 0.90, 0.28)
            } else {
                Color::from_rgba(0.72, 0.76, 0.82, 0.12)
            };
            frame.stroke(
                &path,
                canvas_module::Stroke::default()
                    .with_color(c)
                    .with_width(1.0),
            );
            frame.fill_text(Text {
                content: if db == 0.0 {
                    "0".to_string()
                } else if db.fract().abs() < 0.01 {
                    format!("{db:+.0}")
                } else {
                    format!("{db:+.1}")
                },
                position: Point::new(4.0, y + 2.0),
                color: Color::from_rgba(0.72, 0.76, 0.82, 0.45),
                size: 9.0.into(),
                ..Text::default()
            });
        }

        for hz in [
            20.0_f32, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10_000.0, 20_000.0,
        ] {
            let x = Self::freq_to_x(hz, local_bounds);
            let path = Path::line(Point::new(x, 0.0), Point::new(x, bounds.height));
            frame.stroke(
                &path,
                canvas_module::Stroke::default()
                    .with_color(Color::from_rgba(0.72, 0.76, 0.82, 0.10))
                    .with_width(1.0),
            );
            frame.fill_text(Text {
                content: format_freq(hz),
                position: Point::new(x + 3.0, bounds.height - 14.0),
                color: Color::from_rgba(0.72, 0.76, 0.82, 0.45),
                size: 9.0.into(),
                ..Text::default()
            });
        }

        for (index, marker_data) in self.markers.iter().enumerate() {
            let x = Self::freq_to_x(marker_data.freq_hz, local_bounds);
            let marker = Path::line(Point::new(x, 0.0), Point::new(x, bounds.height));
            let is_draggable = marker_data.on_drag.is_some();
            let is_dragging = state.dragging_marker == Some(index);
            frame.stroke(
                &marker,
                canvas_module::Stroke::default()
                    .with_color(if is_dragging {
                        Color::from_rgba(1.0, 0.90, 0.20, 0.95)
                    } else if is_draggable {
                        Color::from_rgba(1.0, 0.83, 0.10, 0.75)
                    } else {
                        Color::from_rgba(1.0, 0.83, 0.10, 0.55)
                    })
                    .with_width(if is_dragging { 2.0 } else { 1.0 }),
            );
        }

        let channels = if self.stereo { 2 } else { 1 };
        for bins in self.bins_db.iter().take(channels) {
            let fill = self.spectrum_fill_path(bins, local_bounds);
            frame.fill(&fill, Color::from_rgba(0.72, 0.74, 0.78, 0.06));
            let line = self.spectrum_path(bins, local_bounds);
            frame.stroke(
                &line,
                canvas_module::Stroke::default()
                    .with_color(Color::from_rgba(0.72, 0.74, 0.78, 0.32))
                    .with_width(1.2),
            );
        }

        for curve in &self.threshold_curves {
            let path = self.threshold_curve_path(curve, local_bounds);
            let color = if curve.selected {
                curve
                    .selected_color
                    .unwrap_or_else(|| Color::from_rgba(1.0, 0.86, 0.22, 0.96))
            } else {
                curve
                    .color
                    .unwrap_or_else(|| Color::from_rgba(1.0, 0.48, 0.25, 0.72))
            };
            let width = if curve.selected {
                curve.selected_width
            } else {
                curve.width
            };
            frame.stroke(
                &path,
                canvas_module::Stroke::default()
                    .with_color(color)
                    .with_width(width),
            );
        }

        if !self.gain_reduction_db.is_empty() {
            let bar_width = (bounds.width / self.gain_reduction_db.len() as f32).min(90.0);
            let total_width = bar_width * self.gain_reduction_db.len() as f32;
            let start_x = bounds.width - total_width - 12.0;
            for (i, gr) in self.gain_reduction_db.iter().enumerate() {
                let h = ((gr.abs() / 24.0).clamp(0.0, 1.0)) * (bounds.height - 34.0);
                let x = start_x + i as f32 * bar_width;
                let y = bounds.height - 20.0 - h;
                let rect = Path::rectangle(
                    Point::new(x, y),
                    maolan_baseview::iced::Size::new(bar_width - 8.0, h),
                );
                frame.fill(&rect, Color::from_rgba(1.0, 0.35, 0.25, 0.50));
                frame.fill_text(Text {
                    content: format!("{}", i + 1),
                    position: Point::new(x + 4.0, bounds.height - 8.0),
                    color: Color::from_rgba(0.72, 0.76, 0.82, 0.55),
                    size: 9.0.into(),
                    ..Text::default()
                });
            }
        }

        vec![frame.into_geometry()]
    }
}

pub fn format_freq(freq_hz: f32) -> String {
    if freq_hz >= 1000.0 {
        format!("{:.0}k", freq_hz / 1000.0)
    } else {
        format!("{freq_hz:.0}")
    }
}

impl LogSpectrumAnalyzer {
    pub fn new(bins: usize) -> Self {
        let hann: Vec<f32> = (0..FFT_SIZE)
            .map(|i| {
                0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32).cos()
            })
            .collect();
        Self {
            fft: SpectrumAnalyzer::new(FFT_SIZE),
            ring: vec![0.0; FFT_SIZE],
            write_pos: 0,
            hann,
            windowed: vec![0.0; FFT_SIZE],
            mags: vec![0.0; FFT_SIZE / 2 + 1],
            powers: vec![0.0; FFT_SIZE / 2 + 1],
            display_power: vec![0.0; bins],
            smoothed_db: vec![SPECTRUM_FLOOR_DB; bins],
            bins,
        }
    }

    pub fn reset(&mut self) {
        self.ring.fill(0.0);
        self.write_pos = 0;
        self.smoothed_db.fill(SPECTRUM_FLOOR_DB);
    }

    pub fn push_block(&mut self, samples: &[f32]) {
        for &s in samples {
            self.ring[self.write_pos] = s;
            self.write_pos = (self.write_pos + 1) % FFT_SIZE;
        }
    }

    /// Computes the smoothed log-binned spectrum into `out` (dB, floored at
    /// -90). `out.len()` must equal the `bins` this analyzer was created
    /// with. Designed to be called at display rate (~10 Hz), not per block.
    pub fn compute(&mut self, sample_rate: f32, out: &mut [f32]) {
        if out.len() != self.bins || sample_rate <= 0.0 {
            return;
        }
        // Oldest-to-newest copy, Hann windowed.
        for i in 0..FFT_SIZE {
            let idx = (self.write_pos + i) % FFT_SIZE;
            self.windowed[i] = self.ring[idx] * self.hann[i];
        }
        self.fft.process(&self.windowed, &mut self.mags);

        // Hann coherent gain 0.5 and one-sided spectrum: a full-scale sine
        // peaks at its true amplitude with scale 4/N.
        let scale = 4.0 / FFT_SIZE as f32;
        let fft_bins = FFT_SIZE / 2 + 1;
        let bin_hz = sample_rate / FFT_SIZE as f32;
        for (power, mag) in self.powers.iter_mut().zip(self.mags.iter()) {
            let scaled = mag * scale;
            *power = scaled * scaled;
        }

        let f_lo = 20.0_f32;
        let f_hi = (sample_rate * 0.45).min(20_000.0);
        let ratio = f_hi / f_lo;
        let n = self.bins as f32;

        // Per-tick decay so peaks fall smoothly between computes.
        const DECAY_DB_PER_TICK: f32 = 9.0;

        for (i, display_power) in self.display_power.iter_mut().enumerate() {
            let f_edge_lo = f_lo * ratio.powf(i as f32 / n);
            let f_edge_hi = f_lo * ratio.powf((i + 1) as f32 / n);
            let mut k_lo = (f_edge_lo / bin_hz).floor() as usize;
            let mut k_hi = (f_edge_hi / bin_hz).ceil() as usize;
            k_lo = k_lo.min(fft_bins - 1);
            k_hi = k_hi.clamp(k_lo + 1, fft_bins);

            let mut power_sum = 0.0;
            for k in k_lo..k_hi {
                power_sum += self.powers[k];
            }
            *display_power = power_sum;
        }

        for (i, o) in out.iter_mut().enumerate() {
            let start = i.saturating_sub(2);
            let end = (i + 2).min(self.bins - 1);
            let mut weighted_power = 0.0;
            for j in start..=end {
                let distance = i.abs_diff(j) as f32;
                let weight = 3.0 - distance;
                weighted_power += self.display_power[j] * weight;
            }
            let power = weighted_power / 3.0;
            let db = if power > 1.0e-14 {
                10.0 * power.log10()
            } else {
                SPECTRUM_FLOOR_DB
            };
            let smoothed = if db >= self.smoothed_db[i] {
                db
            } else {
                (self.smoothed_db[i] - DECAY_DB_PER_TICK).max(db)
            };
            self.smoothed_db[i] = smoothed.clamp(SPECTRUM_FLOOR_DB, 0.0);
            *o = self.smoothed_db[i];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_scale_sine_peaks_near_zero_dbfs_at_its_bin() {
        let sr = 48_000.0;
        let bins = 192;
        let mut analyzer = LogSpectrumAnalyzer::new(bins);
        let mut pos = 0usize;
        // Feed enough blocks to fill the ring completely.
        while pos < FFT_SIZE * 2 {
            let block: Vec<f32> = (0..512)
                .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * (pos + i) as f32 / sr).sin())
                .collect();
            pos += 512;
            analyzer.push_block(&block);
        }
        let mut out = vec![0.0_f32; bins];
        analyzer.compute(sr, &mut out);
        let peak = out.iter().copied().fold(-200.0_f32, f32::max);
        assert!(peak > -3.5, "peak read {peak} dB");
        assert!(peak < 1.0, "peak read {peak} dB");
        // Bins far from 1 kHz stay well below the peak.
        let bin_hz = |f: f32| {
            let t = (f / 20.0).ln() / (20_000.0_f32 / 20.0).ln();
            (t * bins as f32) as usize
        };
        let at_100 = out[bin_hz(100.0)];
        assert!(at_100 < peak - 40.0, "100 Hz bin at {at_100} dB");
    }
}
