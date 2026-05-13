//! Interactive Bézier envelope editor canvas.

use maolan_baseview::iced::{
    Color, Point, Rectangle, Theme,
    mouse::{self, Cursor},
    widget::canvas::{Action as CanvasAction, Frame, Geometry, Path, Program, Stroke, Text},
};

use crate::kick::dsp::envelope::Envelope;

#[derive(Debug, Clone)]
pub enum EnvelopeEditorMsg {
    /// Point index, new normalized t, new normalized v
    PointMoved(usize, f32, f32),
    /// Point index, is_left_cp, new cp_t, new cp_v
    ControlPointMoved(usize, bool, f32, f32),
    /// New point at normalized t, v
    PointAdded(f32, f32),
    /// Remove point at index
    PointRemoved(usize),
}

pub struct EnvelopeEditor {
    pub envelope: Envelope,
}

pub struct EnvelopeEditorState {
    pub dragging_point: Option<usize>,
    pub dragging_cp: Option<(usize, bool)>, // (point_idx, is_left_cp)
    pub hover_point: Option<usize>,
    pub zoom_x: f32,
    pub offset_x: f32,
}

impl Default for EnvelopeEditorState {
    fn default() -> Self {
        Self {
            dragging_point: None,
            dragging_cp: None,
            hover_point: None,
            zoom_x: 1.0,
            offset_x: 0.0,
        }
    }
}

impl EnvelopeEditor {
    pub fn new(envelope: Envelope) -> Self {
        Self { envelope }
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

    fn draw_curve(&self, state: &EnvelopeEditorState, frame: &mut Frame, width: f32, height: f32) {
        let points = self.envelope.points();
        if points.len() < 2 {
            return;
        }

        let path = Path::new(|builder| {
            let p0 = self.env_to_screen(state, points[0].t, points[0].v, width, height);
            builder.move_to(p0);

            for i in 1..points.len() {
                let prev = &points[i - 1];
                let curr = &points[i];
                let dt = curr.t - prev.t;

                let p_end = self.env_to_screen(state, curr.t, curr.v, width, height);

                // Control points
                let cp0_t = prev.t + prev.cp_t * dt;
                let cp0_v = prev.v + prev.cp_v;
                let cp1_t = curr.t - curr.cp_t * dt;
                let cp1_v = curr.v - curr.cp_v;

                let cp0 = self.env_to_screen(state, cp0_t, cp0_v, width, height);
                let cp1 = self.env_to_screen(state, cp1_t, cp1_v, width, height);

                builder.bezier_curve_to(cp0, cp1, p_end);
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

    fn draw_control_points(
        &self,
        state: &EnvelopeEditorState,
        frame: &mut Frame,
        width: f32,
        height: f32,
    ) {
        let points = self.envelope.points();
        if points.len() < 2 {
            return;
        }

        for i in 0..points.len() {
            let p = &points[i];
            let parent = self.env_to_screen(state, p.t, p.v, width, height);

            // Left control point (from previous segment)
            if i > 0 {
                let prev = &points[i - 1];
                let dt = p.t - prev.t;
                let cp_t = p.t - p.cp_t * dt;
                let cp_v = p.v - p.cp_v;
                let cp_pos = self.env_to_screen(state, cp_t, cp_v, width, height);

                let line = Path::line(parent, cp_pos);
                frame.stroke(
                    &line,
                    Stroke::default()
                        .with_color(Color::from_rgb(0.3, 0.3, 0.35))
                        .with_width(1.0),
                );

                let is_drag = state.dragging_cp == Some((i, true));
                let radius = if is_drag { 4.0 } else { 3.0 };
                let color = if is_drag {
                    Color::from_rgb(0.8, 0.4, 0.2)
                } else {
                    Color::from_rgb(0.5, 0.5, 0.55)
                };
                let circle = Path::circle(cp_pos, radius);
                frame.fill(&circle, color);
            }

            // Right control point (for next segment)
            if i + 1 < points.len() {
                let next = &points[i + 1];
                let dt = next.t - p.t;
                let cp_t = p.t + p.cp_t * dt;
                let cp_v = p.v + p.cp_v;
                let cp_pos = self.env_to_screen(state, cp_t, cp_v, width, height);

                let line = Path::line(parent, cp_pos);
                frame.stroke(
                    &line,
                    Stroke::default()
                        .with_color(Color::from_rgb(0.3, 0.3, 0.35))
                        .with_width(1.0),
                );

                let is_drag = state.dragging_cp == Some((i, false));
                let radius = if is_drag { 4.0 } else { 3.0 };
                let color = if is_drag {
                    Color::from_rgb(0.8, 0.4, 0.2)
                } else {
                    Color::from_rgb(0.5, 0.5, 0.55)
                };
                let circle = Path::circle(cp_pos, radius);
                frame.fill(&circle, color);
            }
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

    fn hit_test_control_point(
        &self,
        state: &EnvelopeEditorState,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Option<(usize, bool)> {
        let points = self.envelope.points();
        if points.len() < 2 {
            return None;
        }

        for i in 0..points.len() {
            let p = &points[i];

            // Left CP
            if i > 0 {
                let prev = &points[i - 1];
                let dt = p.t - prev.t;
                let cp_t = p.t - p.cp_t * dt;
                let cp_v = p.v - p.cp_v;
                let pos = self.env_to_screen(state, cp_t, cp_v, width, height);
                let dx = pos.x - x;
                let dy = pos.y - y;
                if (dx * dx + dy * dy).sqrt() < 6.0 {
                    return Some((i, true));
                }
            }

            // Right CP
            if i + 1 < points.len() {
                let next = &points[i + 1];
                let dt = next.t - p.t;
                let cp_t = p.t + p.cp_t * dt;
                let cp_v = p.v + p.cp_v;
                let pos = self.env_to_screen(state, cp_t, cp_v, width, height);
                let dx = pos.x - x;
                let dy = pos.y - y;
                if (dx * dx + dy * dy).sqrt() < 6.0 {
                    return Some((i, false));
                }
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

        // Background
        frame.fill_rectangle(
            Point::new(0.0, 0.0),
            maolan_baseview::iced::Size::new(width, height),
            Color::from_rgb(0.08, 0.08, 0.10),
        );

        // Grid
        for i in 1..5 {
            let y = height * i as f32 / 5.0;
            let line = Path::line(Point::new(0.0, y), Point::new(width, y));
            frame.stroke(
                &line,
                Stroke::default()
                    .with_color(Color::from_rgb(0.15, 0.15, 0.18))
                    .with_width(0.5),
            );
        }
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

        self.draw_curve(_state, &mut frame, width, height);
        self.draw_control_points(_state, &mut frame, width, height);
        self.draw_points(_state, &mut frame, width, height);

        // Label
        frame.fill_text(Text {
            content: "Envelope".to_string(),
            position: Point::new(8.0, 14.0),
            color: Color::from_rgb(0.7, 0.7, 0.7),
            size: 12.0.into(),
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
                    // Check control points first (smaller, on top)
                    if let Some((idx, is_left)) = self.hit_test_control_point(
                        state,
                        position.x,
                        position.y,
                        bounds.width,
                        bounds.height,
                    ) {
                        state.dragging_cp = Some((idx, is_left));
                        return Some(CanvasAction::request_redraw().and_capture());
                    }
                    // Then check main points
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
                    // Add new point
                    let (t, v) = self.screen_to_env(
                        state,
                        position.x,
                        position.y,
                        bounds.width,
                        bounds.height,
                    );
                    return Some(
                        CanvasAction::publish(EnvelopeEditorMsg::PointAdded(t, v)).and_capture(),
                    );
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
                        CanvasAction::publish(EnvelopeEditorMsg::PointRemoved(idx)).and_capture(),
                    );
                }
            }
            maolan_baseview::iced::Event::Mouse(mouse::Event::ButtonReleased(
                mouse::Button::Left,
            )) => {
                state.dragging_point = None;
                state.dragging_cp = None;
                return Some(CanvasAction::request_redraw());
            }
            maolan_baseview::iced::Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if let Some(position) = cursor.position_in(bounds) {
                    // Update hover
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

                    // Handle dragging
                    if let Some(idx) = state.dragging_point {
                        let (t, v) = self.screen_to_env(
                            state,
                            position.x,
                            position.y,
                            bounds.width,
                            bounds.height,
                        );
                        return Some(CanvasAction::publish(EnvelopeEditorMsg::PointMoved(
                            idx, t, v,
                        )));
                    }
                    if let Some((idx, is_left)) = state.dragging_cp {
                        let (t, v) = self.screen_to_env(
                            state,
                            position.x,
                            position.y,
                            bounds.width,
                            bounds.height,
                        );
                        return Some(CanvasAction::publish(EnvelopeEditorMsg::ControlPointMoved(
                            idx, is_left, t, v,
                        )));
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
