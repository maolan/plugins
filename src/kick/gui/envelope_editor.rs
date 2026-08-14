use maolan_baseview::iced::{
    Color, Point, Rectangle, Theme,
    mouse::{self, Cursor},
    widget::canvas::{Action as CanvasAction, Frame, Geometry, Path, Program, Stroke, Text},
};
use std::time::{Duration, Instant};

use crate::kick::dsp::envelope::Envelope;

fn format_frequency(hz: f32) -> String {
    if hz >= 1000.0 {
        format!("{:.1} kHz", hz / 1000.0)
    } else {
        format!("{hz:.0} Hz")
    }
}

#[derive(Debug, Clone, Copy)]
pub enum EnvelopeScale {
    Normal,
    Frequency { base_hz: f32 },
    Bipolar,
}

#[derive(Debug, Clone)]
pub enum EnvelopeEditorMsg {
    Move(usize, f32, f32),

    Add(f32, f32),

    Remove(usize),
}

pub struct EnvelopeEditor {
    pub envelope: Envelope,
    pub waveform: Option<Vec<f32>>,
    pub length_ms: f32,
    pub scale: EnvelopeScale,
}

pub struct EnvelopeEditorState {
    pub dragging_point: Option<usize>,
    pub hover_point: Option<usize>,
    pub zoom_x: f32,
    pub offset_x: f32,
    pub last_click: Option<(Instant, Point)>,
}

impl Default for EnvelopeEditorState {
    fn default() -> Self {
        Self {
            dragging_point: None,
            hover_point: None,
            zoom_x: 1.0,
            offset_x: 0.0,
            last_click: None,
        }
    }
}

impl EnvelopeEditor {
    pub fn new(
        envelope: Envelope,
        waveform: Option<Vec<f32>>,
        length_ms: f32,
        scale: EnvelopeScale,
    ) -> Self {
        Self {
            envelope,
            waveform,
            length_ms,
            scale,
        }
    }

    fn screen_to_env(
        &self,
        state: &EnvelopeEditorState,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> (f32, f32) {
        let t = (x / width) / state.zoom_x + state.offset_x;
        let v = 1.0 - (y / height);
        (t.clamp(0.0, 1.0), v.clamp(0.0, 1.0))
    }

    fn env_to_screen(
        &self,
        state: &EnvelopeEditorState,
        t: f32,
        v: f32,
        width: f32,
        height: f32,
    ) -> Point {
        let x = ((t - state.offset_x) * state.zoom_x) * width;
        let y = (1.0 - v) * height;
        Point::new(x, y)
    }

    fn draw_waveform(
        &self,
        state: &EnvelopeEditorState,
        frame: &mut Frame,
        width: f32,
        height: f32,
    ) {
        let samples = match self.waveform.as_ref() {
            Some(s) if s.len() >= 2 => s,
            _ => return,
        };

        let path = Path::new(|builder| {
            let len = samples.len();
            for (i, sample) in samples.iter().enumerate() {
                let t = i as f32 / (len - 1) as f32;
                let v = 0.5 + sample.clamp(-1.0, 1.0) * 0.5;
                let x = ((t - state.offset_x) * state.zoom_x) * width;
                let p = Point::new(x, (1.0 - v) * height);
                if i == 0 {
                    builder.move_to(p);
                } else {
                    builder.line_to(p);
                }
            }
        });

        frame.stroke(
            &path,
            Stroke::default()
                .with_color(Color::from_rgb(0.3, 0.7, 0.9))
                .with_width(1.5),
        );
    }

    fn draw_horizontal_grid(&self, frame: &mut Frame, width: f32, height: f32) {
        let lines = match self.scale {
            EnvelopeScale::Bipolar => vec![
                (0.0, Some("+1".to_string()), false),
                (0.25, Some("+0.5".to_string()), false),
                (0.5, Some("0".to_string()), true),
                (0.75, Some("-0.5".to_string()), false),
                (1.0, Some("-1".to_string()), false),
            ],
            EnvelopeScale::Frequency { base_hz } => {
                let base_hz = base_hz.max(0.1);
                [0.0, 0.25, 0.5, 0.75, 1.0]
                    .into_iter()
                    .map(|y| {
                        let env_value = 1.0 - y;
                        let hz = base_hz * env_value;
                        (y, Some(format_frequency(hz)), false)
                    })
                    .collect()
            }
            EnvelopeScale::Normal => (1..5).map(|i| (i as f32 / 5.0, None, false)).collect(),
        };

        for (y_frac, label, major) in lines {
            let y = (height * y_frac).clamp(0.0, height);
            let line = Path::line(Point::new(0.0, y), Point::new(width, y));
            frame.stroke(
                &line,
                Stroke::default()
                    .with_color(if major {
                        Color::from_rgb(0.24, 0.24, 0.29)
                    } else {
                        Color::from_rgb(0.15, 0.15, 0.18)
                    })
                    .with_width(if major { 1.0 } else { 0.5 }),
            );

            if let Some(label) = label {
                frame.fill_text(Text {
                    content: label,
                    position: Point::new(8.0, (y + 4.0).clamp(12.0, height - 4.0)),
                    color: Color::from_rgba(0.72, 0.76, 0.82, 0.55),
                    size: 10.0.into(),
                    font: maolan_baseview::iced::Font::DEFAULT,
                    ..Text::default()
                });
            }
        }
    }

    fn draw_curve(&self, state: &EnvelopeEditorState, frame: &mut Frame, width: f32, height: f32) {
        let points = self.envelope.points();
        if points.len() < 2 {
            return;
        }

        let path = Path::new(|builder| {
            let p0 = self.env_to_screen(state, points[0].t, points[0].v, width, height);
            builder.move_to(p0);

            for curr in points.iter().skip(1) {
                let p_end = self.env_to_screen(state, curr.t, curr.v, width, height);
                builder.line_to(p_end);
            }
        });

        frame.stroke(
            &path,
            Stroke::default()
                .with_color(Color::from_rgb(0.2, 0.85, 0.4))
                .with_width(2.0),
        );
    }

    fn draw_points(&self, state: &EnvelopeEditorState, frame: &mut Frame, width: f32, height: f32) {
        let points = self.envelope.points();
        for (i, p) in points.iter().enumerate() {
            let pos = self.env_to_screen(state, p.t, p.v, width, height);
            let is_hover = state.hover_point == Some(i);
            let is_drag = state.dragging_point == Some(i);

            let radius = if is_drag {
                6.0
            } else if is_hover {
                5.0
            } else {
                4.0
            };
            let color = if is_drag {
                Color::from_rgb(1.0, 0.5, 0.2)
            } else if is_hover {
                Color::from_rgb(0.9, 0.7, 0.3)
            } else {
                Color::from_rgb(0.6, 0.6, 0.7)
            };

            let circle = Path::circle(pos, radius);
            frame.fill(&circle, color);
        }
    }

    fn hit_test_point(
        &self,
        state: &EnvelopeEditorState,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Option<usize> {
        let points = self.envelope.points();
        for (i, p) in points.iter().enumerate() {
            let pos = self.env_to_screen(state, p.t, p.v, width, height);
            let dx = pos.x - x;
            let dy = pos.y - y;
            if (dx * dx + dy * dy).sqrt() < 8.0 {
                return Some(i);
            }
        }
        None
    }
}

impl Program<EnvelopeEditorMsg> for EnvelopeEditor {
    type State = EnvelopeEditorState;

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &maolan_baseview::iced::Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let width = bounds.width;
        let height = bounds.height;

        frame.fill_rectangle(
            Point::new(0.0, 0.0),
            maolan_baseview::iced::Size::new(width, height),
            Color::from_rgb(0.08, 0.08, 0.10),
        );

        self.draw_horizontal_grid(&mut frame, width, height);
        for i in 1..10 {
            let x = width * i as f32 / 10.0;
            let line = Path::line(Point::new(x, 0.0), Point::new(x, height));
            frame.stroke(
                &line,
                Stroke::default()
                    .with_color(Color::from_rgb(0.12, 0.12, 0.15))
                    .with_width(0.5),
            );
        }

        self.draw_waveform(_state, &mut frame, width, height);
        self.draw_curve(_state, &mut frame, width, height);
        self.draw_points(_state, &mut frame, width, height);

        let length_label = if self.length_ms >= 1000.0 {
            format!("{:.2} s", self.length_ms / 1000.0)
        } else {
            format!("{:.0} ms", self.length_ms)
        };
        let label_width = length_label.len() as f32 * 6.5;
        frame.fill_text(Text {
            content: length_label,
            position: Point::new((width - label_width - 8.0).max(8.0), height - 8.0),
            color: Color::from_rgb(0.7, 0.7, 0.7),
            size: 11.0.into(),
            font: maolan_baseview::iced::Font::DEFAULT,
            ..Text::default()
        });

        vec![frame.into_geometry()]
    }

    fn update(
        &self,
        state: &mut Self::State,
        event: &maolan_baseview::iced::Event,
        bounds: Rectangle,
        cursor: Cursor,
    ) -> Option<CanvasAction<EnvelopeEditorMsg>> {
        match event {
            maolan_baseview::iced::Event::Mouse(mouse::Event::ButtonPressed(
                mouse::Button::Left,
            )) => {
                if let Some(position) = cursor.position_in(bounds) {
                    if let Some(idx) = self.hit_test_point(
                        state,
                        position.x,
                        position.y,
                        bounds.width,
                        bounds.height,
                    ) {
                        state.dragging_point = Some(idx);
                        return Some(CanvasAction::request_redraw().and_capture());
                    }

                    let now = Instant::now();
                    let is_double_click = state
                        .last_click
                        .take()
                        .map(|(last_time, last_pos)| {
                            now.duration_since(last_time) <= Duration::from_millis(350)
                                && ((position.x - last_pos.x).powi(2)
                                    + (position.y - last_pos.y).powi(2))
                                .sqrt()
                                    <= 6.0
                        })
                        .unwrap_or(false);
                    let (t, v) = self.screen_to_env(
                        state,
                        position.x,
                        position.y,
                        bounds.width,
                        bounds.height,
                    );
                    if is_double_click {
                        return Some(
                            CanvasAction::publish(EnvelopeEditorMsg::Add(t, v)).and_capture(),
                        );
                    }
                    state.last_click = Some((now, position));
                    return Some(CanvasAction::capture());
                }
            }
            maolan_baseview::iced::Event::Mouse(mouse::Event::ButtonPressed(
                mouse::Button::Right,
            )) => {
                if let Some(position) = cursor.position_in(bounds)
                    && let Some(idx) = self.hit_test_point(
                        state,
                        position.x,
                        position.y,
                        bounds.width,
                        bounds.height,
                    )
                {
                    return Some(
                        CanvasAction::publish(EnvelopeEditorMsg::Remove(idx)).and_capture(),
                    );
                }
            }
            maolan_baseview::iced::Event::Mouse(mouse::Event::ButtonReleased(
                mouse::Button::Left,
            )) => {
                state.dragging_point = None;
                return Some(CanvasAction::request_redraw());
            }
            maolan_baseview::iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let Some(position) = cursor.position_in(bounds) {
                    let hover = self.hit_test_point(
                        state,
                        position.x,
                        position.y,
                        bounds.width,
                        bounds.height,
                    );
                    if state.hover_point != hover {
                        state.hover_point = hover;
                        return Some(CanvasAction::request_redraw());
                    }

                    if let Some(idx) = state.dragging_point {
                        let (t, v) = self.screen_to_env(
                            state,
                            position.x,
                            position.y,
                            bounds.width,
                            bounds.height,
                        );
                        return Some(CanvasAction::publish(EnvelopeEditorMsg::Move(idx, t, v)));
                    }
                }
            }
            maolan_baseview::iced::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                match delta {
                    mouse::ScrollDelta::Lines { x, y: _ }
                    | mouse::ScrollDelta::Pixels { x, y: _ } => {
                        state.offset_x = (state.offset_x - x * 0.05)
                            .clamp(0.0, 1.0 - 1.0 / state.zoom_x.max(1.0));
                        return Some(CanvasAction::request_redraw());
                    }
                }
            }
            _ => {}
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frequency_scale_uses_base_frequency_as_top() {
        let editor = EnvelopeEditor::new(
            Envelope::flat(1.0),
            None,
            100.0,
            EnvelopeScale::Frequency { base_hz: 10000.0 },
        );
        let state = EnvelopeEditorState::default();

        let point = editor.env_to_screen(&state, 0.5, 1.0, 200.0, 100.0);
        assert_eq!(point.x, 100.0);
        assert_eq!(point.y, 0.0);

        let (_, value) = editor.screen_to_env(&state, 100.0, 0.0, 200.0, 100.0);
        assert_eq!(value, 1.0);
    }
}
