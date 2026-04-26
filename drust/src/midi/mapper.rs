use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct MidiMapper {
    /// Map from MIDI note number to instrument name.
    pub map: HashMap<u8, String>,
}

impl MidiMapper {
    pub fn from_xml(xml: &str) -> Result<Self, quick_xml::Error> {
        use quick_xml::events::Event;
        use quick_xml::reader::Reader;

        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);
        let mut mapper = MidiMapper::default();
        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    if e.name().as_ref() == b"map" {
                        let mut note = 0u8;
                        let mut instr = String::new();
                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"note" => {
                                    if let Ok(v) = String::from_utf8_lossy(&attr.value).parse() {
                                        note = v;
                                    }
                                }
                                b"instr" => {
                                    instr = String::from_utf8_lossy(&attr.value).into_owned();
                                }
                                _ => {}
                            }
                        }
                        if !instr.is_empty() {
                            mapper.map.insert(note, instr);
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(e),
                _ => {}
            }
            buf.clear();
        }

        Ok(mapper)
    }

    pub fn instrument_for_note(&self, note: u8) -> Option<&str> {
        self.map.get(&note).map(|s| s.as_str())
    }
}
