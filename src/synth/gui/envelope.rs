use super::{
    Message, ParamId, State, param_control, param_def, param_has_lfo_assignment, visual_param_value,
};
use maolan_widgets::{
    adsr::{AdsrMessage, adsr_envelope},
    iced::{
        Alignment, Element, Length,
        widget::{column, container},
    },
};

pub(super) fn panel(state: &State, ids: [ParamId; 4], mode: ParamId) -> Element<'_, Message> {
    let values = ids.map(|id| visual_param_value(state, id, state.shared.params.get(id) as f32));
    let ranges = ids.map(|id| {
        let def = param_def(id).expect("valid parameter");
        (def.min as f32, def.max as f32)
    });
    let steps = ids.map(|id| param_def(id).expect("valid parameter").step as f32);
    let assigned = ids.map(|id| param_has_lfo_assignment(state, id));
    let graph = adsr_envelope(values, ranges, steps, assigned).map(move |message| {
        let index = match message {
            AdsrMessage::Change(i, _)
            | AdsrMessage::Release(i)
            | AdsrMessage::Reset(i)
            | AdsrMessage::Assign(i) => i,
        };
        let id = ids[index];
        match message {
            AdsrMessage::Change(_, value) => Message::SetParam(id, value),
            AdsrMessage::Release(_) => Message::ReleaseParam(id),
            AdsrMessage::Reset(_) => Message::ResetParam(id),
            AdsrMessage::Assign(_) => Message::AssignLfoToParam(id),
        }
    });
    container(
        column![graph, param_control(mode, "Mode", state)]
            .spacing(3)
            .align_x(Alignment::Center),
    )
    .width(Length::Fixed(220.0))
    .into()
}
