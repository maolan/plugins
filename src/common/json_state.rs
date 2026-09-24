//! Generic JSON parameter-state serialization shared by the standard FX
//! plugins. Plugin-specific state formats (eq, synth, sampler, kick, drums,
//! modeler, stereo section switches, limiter legacy names) stay bespoke.

use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;

use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::common::clap_harness::ParamSpec;
use crate::common::param_store::ParamStore;

const CURRENT_STATE_VERSION: &str = "0.1.0";

fn default_version() -> String {
    CURRENT_STATE_VERSION.to_string()
}

/// Versioned JSON map of parameter names to values, serialized with a
/// plugin-specific text header.
#[derive(Debug, Clone, Serialize)]
pub struct JsonParamState<P: ParamSpec> {
    pub version: String,
    pub params: BTreeMap<String, f64>,
    _phantom: PhantomData<P>,
}

impl<P: ParamSpec> Default for JsonParamState<P> {
    fn default() -> Self {
        Self {
            version: CURRENT_STATE_VERSION.to_string(),
            params: BTreeMap::new(),
            _phantom: PhantomData,
        }
    }
}

impl<P: ParamSpec> JsonParamState<P> {
    pub fn from_runtime(params: &ParamStore<P>) -> Self {
        let mut params_map = BTreeMap::new();
        for def in P::param_defs().iter() {
            params_map.insert(def.name.to_string(), params.get(def.id));
        }
        Self {
            version: CURRENT_STATE_VERSION.to_string(),
            params: params_map,
            _phantom: PhantomData,
        }
    }

    pub fn apply(self, params: &ParamStore<P>) {
        for def in P::param_defs().iter() {
            if let Some(&value) = self.params.get(def.name) {
                params.set(def.id, P::sanitize(def.id, value));
            } else {
                params.set(def.id, def.default);
            }
        }
    }

    pub fn to_bytes(&self, header_prefix: &str) -> Result<Vec<u8>, serde_json::Error> {
        let mut text = format!("{header_prefix}{}\n", self.version);
        text.push_str(&serde_json::to_string(self)?);
        Ok(text.into_bytes())
    }

    pub fn from_bytes(bytes: &[u8], header_prefix: &str) -> Result<Self, String> {
        let text =
            std::str::from_utf8(bytes).map_err(|e| format!("state is not valid UTF-8: {e}"))?;
        let json_text = if let Some(line_end) = text.find('\n') {
            let header = &text[..line_end];
            if header.starts_with(header_prefix) {
                &text[line_end + 1..]
            } else {
                text
            }
        } else {
            text
        };
        serde_json::from_str(json_text).map_err(|e| format!("failed to parse plugin state: {e}"))
    }
}

impl<'de, P: ParamSpec> Deserialize<'de> for JsonParamState<P> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "snake_case")]
        enum Field {
            Version,
            Params,
        }

        struct StateVisitor<P: ParamSpec>(PhantomData<P>);

        impl<'de, P: ParamSpec> Visitor<'de> for StateVisitor<P> {
            type Value = JsonParamState<P>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a JsonParamState map")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut version = None;
                let mut params: Option<BTreeMap<String, f64>> = None;
                while let Some(key) = map.next_key::<Field>()? {
                    match key {
                        Field::Version => {
                            if version.is_some() {
                                return Err(serde::de::Error::duplicate_field("version"));
                            }
                            version = Some(map.next_value::<String>()?);
                        }
                        Field::Params => {
                            if params.is_some() {
                                return Err(serde::de::Error::duplicate_field("params"));
                            }
                            params = Some(map.next_value::<Params<P>>()?.0);
                        }
                    }
                }
                Ok(JsonParamState {
                    version: version.unwrap_or_else(default_version),
                    params: params.unwrap_or_default(),
                    _phantom: PhantomData,
                })
            }
        }

        /// Wrapper accepting either a name->value map or a legacy
        /// index-ordered sequence of values.
        struct Params<P: ParamSpec>(BTreeMap<String, f64>, PhantomData<P>);

        impl<'de, P: ParamSpec> Deserialize<'de> for Params<P> {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                struct ParamsVisitor<P: ParamSpec>(PhantomData<P>);

                impl<'de, P: ParamSpec> Visitor<'de> for ParamsVisitor<P> {
                    type Value = Params<P>;

                    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                        formatter.write_str(
                            "an array of f64 values (legacy index-based) or a map of parameter names to f64 values",
                        )
                    }

                    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
                    where
                        A: SeqAccess<'de>,
                    {
                        let mut map = BTreeMap::new();
                        let mut index = 0usize;
                        while let Some(value) = seq.next_element::<f64>()? {
                            if let Some(def) = P::param_defs().get(index) {
                                map.insert(def.name.to_string(), value);
                            }
                            index += 1;
                        }
                        Ok(Params(map, PhantomData))
                    }

                    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
                    where
                        A: MapAccess<'de>,
                    {
                        let mut result = BTreeMap::new();
                        while let Some((key, value)) = map.next_entry::<String, f64>()? {
                            result.insert(key, value);
                        }
                        Ok(Params(result, PhantomData))
                    }
                }

                deserializer.deserialize_any(ParamsVisitor::<P>(PhantomData))
            }
        }

        deserializer.deserialize_map(StateVisitor::<P>(PhantomData))
    }
}
