use std::ffi::c_char;

use clap_clap::{
    events::{EventBuilder, InputEvents, OutputEvents, ParamValue},
    ffi::{CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_PARAM_VALUE},
    id::ClapId,
};

/// Trait for parameter ID enums used across CLAP plugins.
pub trait ClapParamId: Copy + Clone + PartialEq + Eq + Send + Sync + 'static {
    const COUNT: usize;
    fn as_index(self) -> usize;
    fn from_raw(id: u32) -> Option<Self>;
}

/// Minimal trait that a plugin's `SharedState` must implement to use
/// the generic param-event helpers.
pub trait SharedStateExt<P: ClapParamId> {
    fn params_get(&self, id: P) -> f64;
    fn has_local_param_override(&self, id: P) -> bool;
    fn clear_local_param_override(&self, id: P);
    fn set_param_from_host(&self, id: P, value: f64);
    fn take_pending_param_notifications(&self) -> u32;
    fn requeue_pending_param_notifications(&self, bits: u32);
}

impl<P: ClapParamId, S: SharedStateExt<P>> SharedStateExt<P> for std::sync::Arc<S> {
    fn params_get(&self, id: P) -> f64 {
        (**self).params_get(id)
    }
    fn has_local_param_override(&self, id: P) -> bool {
        (**self).has_local_param_override(id)
    }
    fn clear_local_param_override(&self, id: P) {
        (**self).clear_local_param_override(id);
    }
    fn set_param_from_host(&self, id: P, value: f64) {
        (**self).set_param_from_host(id, value);
    }
    fn take_pending_param_notifications(&self) -> u32 {
        (**self).take_pending_param_notifications()
    }
    fn requeue_pending_param_notifications(&self, bits: u32) {
        (**self).requeue_pending_param_notifications(bits);
    }
}

pub fn copy_str_to_array<const N: usize>(source: &str, target: &mut [c_char; N]) {
    target.fill(0);
    for (dst, src) in target.iter_mut().zip(source.as_bytes().iter().copied()) {
        *dst = src as c_char;
    }
}

pub fn apply_param_events<P: ClapParamId, S: SharedStateExt<P>>(
    shared: &S,
    events: &InputEvents<'_>,
    sanitize: impl Fn(P, f64) -> f64,
) {
    for index in 0..events.size() {
        let header = events.get(index);
        if header.space_id() != CLAP_CORE_EVENT_SPACE_ID {
            continue;
        }
        if header.r#type() != CLAP_EVENT_PARAM_VALUE as u16 {
            continue;
        }
        if let Ok(param) = header.param_value() {
            let raw: u32 = param.param_id().into();
            if let Some(id) = P::from_raw(raw) {
                let incoming = sanitize(id, param.value());
                if shared.has_local_param_override(id) {
                    let current = shared.params_get(id);
                    if (incoming - current).abs() > 1.0e-9 {
                        continue;
                    }
                    shared.clear_local_param_override(id);
                }
                shared.set_param_from_host(id, incoming);
            }
        }
    }
}

pub fn emit_pending_param_events_to_host<P: ClapParamId, S: SharedStateExt<P>>(
    shared: &S,
    out_events: &mut OutputEvents<'_>,
) {
    let pending = shared.take_pending_param_notifications();
    if pending == 0 {
        return;
    }

    let mut failed = 0_u32;
    for id in (0..P::COUNT).filter_map(|i| P::from_raw(i as u32)) {
        let bit = 1_u32 << (id.as_index() as u32);
        if pending & bit == 0 {
            continue;
        }
        let event_builder = ParamValue::build()
            .param_id(ClapId::from(id.as_index() as u16))
            .value(shared.params_get(id));
        let event = event_builder.event();
        if out_events.try_push(event).is_err() {
            failed |= bit;
        }
    }

    shared.requeue_pending_param_notifications(failed);
}
