use std::sync::Arc;

use maolan_baseview::iced::{
    Color, Element, Length, Point, Rectangle, Renderer, Theme, mouse,
    widget::{
        Space, Stack, canvas,
        canvas::{Frame, Geometry, Path, Program, Stroke},
        container, mouse_area, row, text,
    },
};

use crate::{
    common::{
        filter::FilterType,
        wavetable_factory::{CUSTOM_WAVETABLE_LABEL, FACTORY_COUNT, FACTORY_NAMES, factory_table},
    },
    synth::{
        dsp::{
            AliasWaveform, ClassicWaveform, ExciterType, NoiseColorMode, NoiseGenerator, NoiseType,
            OscType, Oscillator, WindowType,
        },
        params::ParamId,
        plugin::SharedState,
    },
};

const SAMPLE_RATE: f32 = 48_000.0;
const POINT_COUNT: usize = 512;

pub struct OscillatorDisplay {
    shared: Arc<SharedState>,
    osc_index: usize,
}

impl OscillatorDisplay {
    pub fn new(shared: Arc<SharedState>, osc_index: usize) -> Self {
        Self { shared, osc_index }
    }

    pub fn view(self) -> Element<'static, super::Message> {
        let (osc_type, waveform) = display_labels(&self.shared, self.osc_index);
        let type_shared = self.shared.clone();
        let type_label: Element<'static, super::Message> = if self.osc_index == 3 {
            text(osc_type).size(11).into()
        } else {
            mouse_area(text(osc_type).size(11))
                .on_scroll(move |delta| scroll_osc_type(&type_shared, self.osc_index, delta))
                .into()
        };
        let waveform_shared = self.shared.clone();
        let waveform_label = mouse_area(text(waveform).size(11))
            .on_scroll(move |delta| scroll_waveform(&waveform_shared, self.osc_index, delta));
        let waveform_canvas = canvas(self)
            .width(Length::Fixed(220.0))
            .height(Length::Fill)
            .into();
        let labels = container(row![
            type_label,
            Space::new().width(Length::Fill),
            waveform_label,
        ])
        .width(Length::Fill)
        .padding(6);

        Stack::with_children(vec![waveform_canvas, labels.into()]).into()
    }
}

impl<Message> Program<Message> for OscillatorDisplay {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: maolan_baseview::iced::mouse::Cursor,
    ) -> Vec<Geometry> {
        let samples = render_samples(&self.shared, self.osc_index);
        let mut frame = Frame::new(renderer, bounds.size());
        frame.fill_rectangle(
            Point::ORIGIN,
            bounds.size(),
            Color::from_rgb(0.075, 0.09, 0.115),
        );

        let center_y = bounds.height * 0.5;
        let center = Path::line(
            Point::new(0.0, center_y),
            Point::new(bounds.width, center_y),
        );
        frame.stroke(
            &center,
            Stroke::default()
                .with_color(Color::from_rgba(0.55, 0.60, 0.66, 0.22))
                .with_width(1.0),
        );

        let path = Path::new(|builder| {
            for (index, sample) in samples.iter().copied().enumerate() {
                let x = index as f32 / (POINT_COUNT - 1) as f32 * bounds.width;
                let y = center_y - sample.clamp(-1.0, 1.0) * center_y * 0.9;
                if index == 0 {
                    builder.move_to(Point::new(x, y));
                } else {
                    builder.line_to(Point::new(x, y));
                }
            }
        });
        frame.stroke(
            &path,
            Stroke::default()
                .with_color(Color::from_rgb(0.72, 0.58, 0.28))
                .with_width(1.6),
        );

        vec![frame.into_geometry()]
    }
}

fn scroll_osc_type(
    shared: &SharedState,
    osc_index: usize,
    delta: mouse::ScrollDelta,
) -> super::Message {
    let type_id = match osc_index {
        0 => ParamId::Osc1Type,
        1 => ParamId::Osc2Type,
        _ => ParamId::Osc3Type,
    };
    let current = OscType::from_u8(shared.params.get(type_id) as u8);
    let current_index = super::OSC_TYPES
        .iter()
        .position(|&osc_type| osc_type == current)
        .unwrap_or(0);
    let next_index = offset_index(
        current_index,
        super::OSC_TYPES.len(),
        scroll_direction(delta),
    );
    super::Message::SetParam(type_id, super::OSC_TYPES[next_index] as u8 as f32)
}

fn scroll_waveform(
    shared: &SharedState,
    osc_index: usize,
    delta: mouse::ScrollDelta,
) -> super::Message {
    let direction = scroll_direction(delta);
    if osc_index == 3 {
        let current = shared.params.get(ParamId::NoiseType) as usize;
        let next = offset_index(current, 3, direction);
        return super::Message::SetParam(ParamId::NoiseType, next as f32);
    }

    let (type_id, waveform_id, shape_id, wavetable_id) = match osc_index {
        0 => (
            ParamId::Osc1Type,
            ParamId::Osc1Waveform,
            ParamId::Osc1Shape,
            ParamId::Osc1Wavetable,
        ),
        1 => (
            ParamId::Osc2Type,
            ParamId::Osc2Waveform,
            ParamId::Osc2Shape,
            ParamId::Osc2Wavetable,
        ),
        _ => (
            ParamId::Osc3Type,
            ParamId::Osc3Waveform,
            ParamId::Osc3Shape,
            ParamId::Osc3Wavetable,
        ),
    };
    let osc_type = OscType::from_u8(shared.params.get(type_id) as u8);
    if osc_type == OscType::Wavetable {
        let current = shared.params.get(wavetable_id) as usize;
        let next = offset_index(current, FACTORY_COUNT, direction);
        return super::Message::SetParam(wavetable_id, next as f32);
    }

    let options = super::waveform_options(osc_type);
    let current = if osc_type == OscType::Twist {
        let shape = shared.params.get(shape_id) as f32;
        let model = ((shape.clamp(0.0, 1.0) * 16.0) as usize).min(15);
        if model >= 4 { model - 1 } else { model }
    } else {
        shared.params.get(waveform_id) as usize
    };
    let next = offset_index(current, options.len(), direction);
    if osc_type == OscType::Twist {
        let model = if next >= 4 { next + 1 } else { next };
        super::Message::SetParam(shape_id, model as f32 / 16.0)
    } else {
        super::Message::SetParam(waveform_id, next as f32)
    }
}

fn scroll_direction(delta: mouse::ScrollDelta) -> isize {
    let y = match delta {
        mouse::ScrollDelta::Lines { y, .. } | mouse::ScrollDelta::Pixels { y, .. } => y,
    };
    if y < 0.0 {
        1
    } else if y > 0.0 {
        -1
    } else {
        0
    }
}

fn offset_index(current: usize, len: usize, direction: isize) -> usize {
    let current = current.min(len - 1);
    if direction < 0 {
        current.saturating_sub(1)
    } else if direction > 0 {
        (current + 1).min(len - 1)
    } else {
        current
    }
}

fn display_labels(shared: &SharedState, osc_index: usize) -> (String, String) {
    if osc_index == 3 {
        let noise_type = match NoiseType::from_u8(shared.params.get(ParamId::NoiseType) as u8) {
            NoiseType::White => "White",
            NoiseType::Pink => "Pink",
            NoiseType::Brown => "Brown",
        };
        return ("Noise".to_string(), noise_type.to_string());
    }

    let (type_id, waveform_id, shape_id, wavetable_id) = match osc_index {
        0 => (
            ParamId::Osc1Type,
            ParamId::Osc1Waveform,
            ParamId::Osc1Shape,
            ParamId::Osc1Wavetable,
        ),
        1 => (
            ParamId::Osc2Type,
            ParamId::Osc2Waveform,
            ParamId::Osc2Shape,
            ParamId::Osc2Wavetable,
        ),
        _ => (
            ParamId::Osc3Type,
            ParamId::Osc3Waveform,
            ParamId::Osc3Shape,
            ParamId::Osc3Wavetable,
        ),
    };
    let osc_type = OscType::from_u8(shared.params.get(type_id) as u8);
    let options = super::waveform_options(osc_type);
    let selected_index = if osc_type == OscType::Twist {
        let shape = shared.params.get(shape_id) as f32;
        let model = ((shape.clamp(0.0, 1.0) * 16.0) as usize).min(15);
        if model >= 4 { model - 1 } else { model }
    } else {
        shared.params.get(waveform_id) as usize
    };
    let waveform = if osc_type == OscType::Wavetable {
        let table_index = shared.params.get(wavetable_id) as usize;
        if table_index >= FACTORY_COUNT {
            CUSTOM_WAVETABLE_LABEL
        } else {
            FACTORY_NAMES[table_index]
        }
    } else {
        options.get(selected_index).copied().unwrap_or("")
    };

    (osc_type.to_string(), waveform.to_string())
}

fn render_samples(shared: &SharedState, osc_index: usize) -> Vec<f32> {
    if osc_index == 3 {
        return render_noise_samples(shared);
    }

    let (type_id, waveform_id, shape_id, skew_id, formant_id, wavetable_id) = match osc_index {
        0 => (
            ParamId::Osc1Type,
            ParamId::Osc1Waveform,
            ParamId::Osc1Shape,
            ParamId::Osc1Skew,
            ParamId::Osc3Formant,
            ParamId::Osc1Wavetable,
        ),
        1 => (
            ParamId::Osc2Type,
            ParamId::Osc2Waveform,
            ParamId::Osc2Shape,
            ParamId::Osc2Skew,
            ParamId::Osc3Formant,
            ParamId::Osc2Wavetable,
        ),
        _ => (
            ParamId::Osc3Type,
            ParamId::Osc3Waveform,
            ParamId::Osc3Shape,
            ParamId::Osc3Skew,
            ParamId::Osc3Formant,
            ParamId::Osc3Wavetable,
        ),
    };
    let get = |id| shared.params.get(id) as f32;
    let osc_type = OscType::from_u8(get(type_id) as u8);
    let waveform = get(waveform_id) as u8;
    let mut oscillator = Oscillator::new(osc_type, SAMPLE_RATE);
    oscillator.reset_to_zero();
    oscillator.set_unison(1, 0.0);
    oscillator.set_freq_hz(SAMPLE_RATE / POINT_COUNT as f32);
    oscillator.set_shape(get(shape_id));
    oscillator.set_skew(get(skew_id));
    oscillator.set_formant(get(formant_id));

    match &mut oscillator {
        Oscillator::Classic(osc) => osc.set_waveform(ClassicWaveform::from_u8(waveform)),
        Oscillator::Window(osc) => osc.set_window_type(WindowType::from_u8(waveform)),
        Oscillator::String(osc) => osc.set_exciter(ExciterType::from_u8(waveform)),
        Oscillator::Alias(osc) => osc.set_waveform(AliasWaveform::from_u8(waveform)),
        _ => {}
    }

    let table_index = get(wavetable_id) as usize;
    let wavetable = if table_index >= FACTORY_COUNT {
        shared.custom_wavetables[osc_index].lock().clone()
    } else {
        factory_table(table_index)
    };
    oscillator.set_wavetable(wavetable.or_else(|| factory_table(0)));

    (0..POINT_COUNT)
        .map(|_| {
            let (left, right) = oscillator.next(0.0, 0.0, 0.0);
            (left + right) * 0.5
        })
        .collect()
}

fn render_noise_samples(shared: &SharedState) -> Vec<f32> {
    let get = |id| shared.params.get(id) as f32;
    let mut noise = NoiseGenerator::new(SAMPLE_RATE);
    noise.noise_type = NoiseType::from_u8(get(ParamId::NoiseType) as u8);
    noise.color = get(ParamId::NoiseColor);
    noise.color_mode = if get(ParamId::NoiseColorMode) == 0.0 {
        NoiseColorMode::Tilt
    } else {
        NoiseColorMode::Legacy
    };
    noise.filter_enabled = get(ParamId::NoiseFilterEnabled) > 0.5;
    if noise.filter_enabled {
        let cutoff = get(ParamId::NoiseFilterCutoff);
        let resonance = get(ParamId::NoiseFilterResonance);
        noise
            .filter
            .set_filter_type(FilterType::from_u8(get(ParamId::NoiseFilterType) as u8));
        noise.filter.set_params(cutoff, resonance);
        noise.filter.prepare_block(cutoff, resonance, 1);
    }

    (0..POINT_COUNT).map(|_| noise.next()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_saw_preview_is_finite_and_nonzero() {
        let shared = SharedState::default();
        let samples = render_samples(&shared, 0);
        assert_eq!(samples.len(), POINT_COUNT);
        assert!(samples.iter().all(|sample| sample.is_finite()));
        let min = samples.iter().copied().fold(f32::INFINITY, f32::min);
        let max = samples.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(max - min > 0.5, "unexpected preview range: {min}..{max}");
    }
}
