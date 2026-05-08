use std::ffi::c_char;

use clap_clap::{
    events::{EventBuilder, InputEvents, OutputEvents, ParamValue},
    ffi::{
        CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_PARAM_GESTURE_BEGIN, CLAP_EVENT_PARAM_GESTURE_END,
        CLAP_EVENT_PARAM_VALUE, clap_event_header, clap_event_param_gesture,
    },
    id::ClapId,
};
use std::mem::size_of;

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
    fn set_gesture_active(&self, id: P, active: bool);
    fn is_gesture_active(&self, id: P) -> bool;
    fn set_param_from_host(&self, id: P, value: f64);
    fn take_pending_param_notifications(&self) -> u32;
    fn requeue_pending_param_notifications(&self, bits: u32);
    fn take_pending_gesture_begin(&self) -> u32;
    fn requeue_pending_gesture_begin(&self, bits: u32);
    fn take_pending_gesture_end(&self) -> u32;
    fn requeue_pending_gesture_end(&self, bits: u32);
}

impl<P: ClapParamId, S: SharedStateExt<P>> SharedStateExt<P> for std::sync::Arc<S> {
    fn params_get(&self, id: P) -> f64 {
        (**self).params_get(id)
    }
    fn set_gesture_active(&self, id: P, active: bool) {
        (**self).set_gesture_active(id, active);
    }
    fn is_gesture_active(&self, id: P) -> bool {
        (**self).is_gesture_active(id)
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
    fn take_pending_gesture_begin(&self) -> u32 {
        (**self).take_pending_gesture_begin()
    }
    fn requeue_pending_gesture_begin(&self, bits: u32) {
        (**self).requeue_pending_gesture_begin(bits);
    }
    fn take_pending_gesture_end(&self) -> u32 {
        (**self).take_pending_gesture_end()
    }
    fn requeue_pending_gesture_end(&self, bits: u32) {
        (**self).requeue_pending_gesture_end(bits);
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
        match header.r#type() {
            t if t == CLAP_EVENT_PARAM_GESTURE_BEGIN as u16 => {
                let gesture = unsafe {
                    &*((header.as_clap_event_header() as *const clap_event_header)
                        as *const clap_event_param_gesture)
                };
                if let Some(id) = P::from_raw(gesture.param_id) {
                    shared.set_gesture_active(id, true);
                }
            }
            t if t == CLAP_EVENT_PARAM_GESTURE_END as u16 => {
                let gesture = unsafe {
                    &*((header.as_clap_event_header() as *const clap_event_header)
                        as *const clap_event_param_gesture)
                };
                if let Some(id) = P::from_raw(gesture.param_id) {
                    shared.set_gesture_active(id, false);
                }
            }
            t if t == CLAP_EVENT_PARAM_VALUE as u16 => {
                if let Ok(param) = header.param_value() {
                    let raw: u32 = param.param_id().into();
                    if let Some(id) = P::from_raw(raw) {
                        if shared.is_gesture_active(id) {
                            continue;
                        }
                        let incoming = sanitize(id, param.value());
                        shared.set_param_from_host(id, incoming);
                    }
                }
            }
            _ => {}
        }
    }
}

pub fn emit_pending_param_events_to_host<P: ClapParamId, S: SharedStateExt<P>>(
    shared: &S,
    out_events: &mut OutputEvents<'_>,
) {
    let pending_begin = shared.take_pending_gesture_begin();
    let pending_values = shared.take_pending_param_notifications();
    let pending_end = shared.take_pending_gesture_end();

    if pending_begin == 0 && pending_values == 0 && pending_end == 0 {
        return;
    }

    let mut failed_begin = 0_u32;
    let mut failed_values = 0_u32;
    let mut failed_end = 0_u32;
    for id in (0..P::COUNT).filter_map(|i| P::from_raw(i as u32)) {
        let bit = 1_u32 << (id.as_index() as u32);
        if pending_begin & bit != 0 {
            let begin = ParamGesture::begin(ClapId::from(id.as_index() as u16));
            if out_events.try_push(begin).is_err() {
                failed_begin |= bit;
            }
        }
        if pending_values & bit != 0 {
            let event_builder = ParamValue::build()
                .param_id(ClapId::from(id.as_index() as u16))
                .value(shared.params_get(id));
            let event = event_builder.event();
            if out_events.try_push(event).is_err() {
                failed_values |= bit;
            }
        }
        if pending_end & bit != 0 {
            let end = ParamGesture::end(ClapId::from(id.as_index() as u16));
            if out_events.try_push(end).is_err() {
                failed_end |= bit;
            }
        }
    }

    shared.requeue_pending_gesture_begin(failed_begin);
    shared.requeue_pending_param_notifications(failed_values);
    shared.requeue_pending_gesture_end(failed_end);
}

#[derive(Debug, Copy, Clone)]
struct ParamGesture {
    inner: clap_event_param_gesture,
}

impl ParamGesture {
    fn begin(id: ClapId) -> Self {
        Self::new(id, CLAP_EVENT_PARAM_GESTURE_BEGIN as u16)
    }

    fn end(id: ClapId) -> Self {
        Self::new(id, CLAP_EVENT_PARAM_GESTURE_END as u16)
    }

    fn new(id: ClapId, event_type: u16) -> Self {
        Self {
            inner: clap_event_param_gesture {
                header: clap_event_header {
                    size: size_of::<clap_event_param_gesture>() as u32,
                    time: 0,
                    space_id: CLAP_CORE_EVENT_SPACE_ID,
                    r#type: event_type,
                    flags: 0,
                },
                param_id: id.into(),
            },
        }
    }
}

impl clap_clap::events::Event for ParamGesture {
    fn header(&self) -> &clap_clap::events::Header {
        unsafe { clap_clap::events::Header::new_unchecked(&self.inner.header) }
    }
}
