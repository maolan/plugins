use super::{Message, ParamId, State, param_def, param_has_lfo_assignment, visual_param_value};
use maolan_widgets::{
    adsr::{AdsrMessage, adsr_envelope_with_edit},
    iced::{Element, Length, widget::container},
};

pub(super) fn panel(state: &State, ids: [ParamId; 4]) -> Element<'_, Message> {
    let values = ids.map(|id| visual_param_value(state, id, state.shared.params.get(id) as f32));
    let ranges = ids.map(|id| {
        let def = param_def(id).expect("valid parameter");
        (def.min as f32, def.max as f32)
    });
    let steps = ids.map(|id| param_def(id).expect("valid parameter").step as f32);
    let assigned = ids.map(|id| param_has_lfo_assignment(state, id));
    let editing_index = state
        .envelope_edit
        .as_ref()
        .and_then(|(id, _)| ids.iter().position(|candidate| candidate == id));
    let edit_value = state
        .envelope_edit
        .as_ref()
        .map_or("", |(_, value)| value.as_str());
    let graph = adsr_envelope_with_edit(
        values,
        ranges,
        steps,
        assigned,
        editing_index,
        edit_value,
        state.envelope_input_id.clone(),
    )
    .map(move |message| {
        let index = match &message {
            AdsrMessage::Change(i, _)
            | AdsrMessage::Release(i)
            | AdsrMessage::Reset(i)
            | AdsrMessage::Assign(i)
            | AdsrMessage::Edit(i)
            | AdsrMessage::EditChanged(i, _)
            | AdsrMessage::EditSubmit(i) => *i,
        };
        let id = ids[index];
        match message {
            AdsrMessage::Change(_, value) => Message::SetParam(id, value),
            AdsrMessage::Release(_) => Message::ReleaseParam(id),
            AdsrMessage::Reset(_) => Message::ResetParam(id),
            AdsrMessage::Assign(_) => Message::AssignLfoToParam(id),
            AdsrMessage::Edit(_) => Message::BeginEnvelopeEdit(id),
            AdsrMessage::EditChanged(_, value) => Message::ChangeEnvelopeEdit(id, value),
            AdsrMessage::EditSubmit(_) => Message::CommitEnvelopeEdit(id),
        }
    });
    container(graph).width(Length::Fixed(220.0)).into()
}
