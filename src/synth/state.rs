use super::params::{AMP_ENVELOPE_IDS, OSC_ENVELOPE_IDS, ParamStore};
pub use crate::common::state::PluginState;

pub fn apply_synth_state(state: &PluginState, store: &ParamStore) {
    state.apply(store);
    for ids in OSC_ENVELOPE_IDS {
        for (id, legacy) in ids.into_iter().zip(AMP_ENVELOPE_IDS) {
            if state.params.len() <= id.as_index() {
                store.set(id, store.get(legacy));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_state_initializes_each_oscillator_envelope() {
        let source = ParamStore::default();
        for (id, value) in AMP_ENVELOPE_IDS.into_iter().zip([0.1, 0.2, 0.3, 0.4]) {
            source.set(id, value);
        }
        let mut state = PluginState::from_runtime(&source);
        state.params.truncate(720);
        let restored = ParamStore::default();
        for ids in OSC_ENVELOPE_IDS {
            for id in ids {
                restored.set(id, 0.9);
            }
        }
        apply_synth_state(&state, &restored);
        for ids in OSC_ENVELOPE_IDS {
            for (id, legacy) in ids.into_iter().zip(AMP_ENVELOPE_IDS) {
                assert_eq!(restored.get(id), source.get(legacy));
            }
        }
    }

    #[test]
    fn independent_envelopes_survive_state_roundtrip() {
        let source = ParamStore::default();
        for (i, ids) in OSC_ENVELOPE_IDS.into_iter().enumerate() {
            for (stage, id) in ids.into_iter().enumerate() {
                source.set(id, (i * 4 + stage) as f64 / 20.0);
            }
        }
        let state =
            PluginState::from_bytes(&PluginState::from_runtime(&source).to_bytes().unwrap())
                .unwrap();
        let restored = ParamStore::default();
        apply_synth_state(&state, &restored);
        for ids in OSC_ENVELOPE_IDS {
            for id in ids {
                assert_eq!(restored.get(id), source.get(id));
            }
        }
    }
}
