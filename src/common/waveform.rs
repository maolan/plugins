use maolan_baseview::iced::{
    Color, Element, Length, Point, Rectangle, Renderer, Theme,
    widget::{
        canvas,
        canvas::{Frame, Geometry, Path, Program, Stroke},
    },
};

#[derive(Clone)]
pub struct ScrollingWaveformWidget<const POINTS: usize> {
    samples: [f32; POINTS],
}

impl<const POINTS: usize> ScrollingWaveformWidget<POINTS> {
    pub fn new(samples: [f32; POINTS]) -> Self {
        Self { samples }
    }

    pub fn view<Message: 'static>(self) -> Element<'static, Message> {
        canvas(self).width(Length::Fill).height(Length::Fill).into()
    }
}

impl<Message, const POINTS: usize> Program<Message> for ScrollingWaveformWidget<POINTS> {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: maolan_baseview::iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let local = Rectangle {
            x: 0.0,
            y: 0.0,
            width: bounds.width,
            height: bounds.height,
        };

        frame.fill_rectangle(
            Point::new(0.0, 0.0),
            local.size(),
            Color::from_rgb(0.055, 0.065, 0.09),
        );

        draw_grid(&mut frame, local);
        draw_waveform(&mut frame, local, &self.samples);

        vec![frame.into_geometry()]
    }
}

fn draw_grid(frame: &mut Frame, bounds: Rectangle) {
    let grid_color = Color::from_rgba(0.62, 0.68, 0.78, 0.12);
    let center_color = Color::from_rgba(0.82, 0.87, 0.94, 0.28);

    for i in 1..4 {
        let y = bounds.y + bounds.height * i as f32 / 4.0;
        let line = Path::line(
            Point::new(bounds.x, y),
            Point::new(bounds.x + bounds.width, y),
        );
        frame.stroke(
            &line,
            Stroke::default().with_color(grid_color).with_width(1.0),
        );
    }

    for i in 1..8 {
        let x = bounds.x + bounds.width * i as f32 / 8.0;
        let line = Path::line(
            Point::new(x, bounds.y),
            Point::new(x, bounds.y + bounds.height),
        );
        frame.stroke(
            &line,
            Stroke::default().with_color(grid_color).with_width(1.0),
        );
    }

    let center_y = bounds.y + bounds.height * 0.5;
    let center = Path::line(
        Point::new(bounds.x, center_y),
        Point::new(bounds.x + bounds.width, center_y),
    );
    frame.stroke(
        &center,
        Stroke::default().with_color(center_color).with_width(1.0),
    );
}

fn draw_waveform<const POINTS: usize>(
    frame: &mut Frame,
    bounds: Rectangle,
    samples: &[f32; POINTS],
) {
    if POINTS < 2 {
        return;
    }

    let center_y = bounds.y + bounds.height * 0.5;
    let amp = bounds.height * 0.45;
    let path = Path::new(|builder| {
        for (i, sample) in samples.iter().copied().enumerate() {
            let t = i as f32 / (POINTS - 1) as f32;
            let x = bounds.x + t * bounds.width;
            let y = center_y - sample.clamp(-1.2, 1.2) / 1.2 * amp;
            if i == 0 {
                builder.move_to(Point::new(x, y));
            } else {
                builder.line_to(Point::new(x, y));
            }
        }
    });

    frame.stroke(
        &path,
        Stroke::default()
            .with_color(Color::from_rgb(0.95, 0.96, 0.98))
            .with_width(1.6),
    );
}
