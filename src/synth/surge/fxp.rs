//! Surge XT `.fxp` preset container parsing.
//!
//! A Surge `.fxp` file is a Steinberg *chunked* FXP preset whose payload is
//! Surge's own little-endian `sub3` blob: a 32-byte header followed by the
//! patch XML. Reference: `src/common/PatchFileHeaderStructs.h` and
//! `SurgeSynthesizerIO.cpp` in the Surge source.

/// Parsed container of a Surge `.fxp` file.
#[derive(Debug)]
pub struct SurgePatchFile {
    /// Patch name from the FXP program header.
    pub name: String,
    /// The `<patch>` XML document.
    pub xml: String,
    /// `true` for each scene/osc slot (index = scene * 3 + osc) whose patch
    /// embeds a wavetable blob we do not convert.
    pub embedded_wavetables: [bool; 6],
}

fn be_u32(data: &[u8], offset: usize, what: &str) -> Result<u32, String> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or_else(|| format!("file too short while reading {what}"))?;
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn le_u32(data: &[u8], offset: usize, what: &str) -> Result<u32, String> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or_else(|| format!("file too short while reading {what}"))?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

const CHUNK_MAGIC: u32 = u32::from_be_bytes(*b"CcnK");
const FX_MAGIC_CHUNK: u32 = u32::from_be_bytes(*b"FPCh");
const SURGE_FX_ID: u32 = u32::from_be_bytes(*b"cjs3");
const INNER_TAG: &[u8; 4] = b"sub3";

/// Parse a Surge XT `.fxp` file into its XML document.
pub fn parse_fxp(data: &[u8]) -> Result<SurgePatchFile, String> {
    // Outer Steinberg header, 60 bytes, big-endian.
    if be_u32(data, 0, "chunk magic")? != CHUNK_MAGIC {
        return Err("not an FXP file (missing CcnK magic)".to_string());
    }
    if be_u32(data, 8, "fx magic")? != FX_MAGIC_CHUNK {
        return Err("not a chunked FXP preset (expected FPCh); Surge parameter-dump presets are unsupported".to_string());
    }
    if be_u32(data, 16, "fx id")? != SURGE_FX_ID {
        return Err("not a Surge patch (fxID is not 'cjs3')".to_string());
    }
    let name_bytes = data
        .get(28..56)
        .ok_or_else(|| "file too short while reading program name".to_string())?;
    let name_end = name_bytes
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(name_bytes.len());
    let name = String::from_utf8_lossy(&name_bytes[..name_end]).into_owned();
    let chunk_size = be_u32(data, 56, "chunk size")? as usize;
    let payload = data
        .get(60..60 + chunk_size)
        .ok_or_else(|| format!("file truncated: chunk size {chunk_size} exceeds file"))?;

    // Inner Surge blob. If the payload does not carry the `sub3` tag, Surge
    // treats the whole payload as a raw XML document; accept that too.
    if payload.get(0..4) != Some(INNER_TAG.as_slice()) {
        let xml = String::from_utf8_lossy(payload).into_owned();
        return Ok(SurgePatchFile {
            name,
            xml,
            embedded_wavetables: [false; 6],
        });
    }

    let xml_size = le_u32(payload, 4, "xml size")? as usize;
    let mut embedded = [false; 6];
    for (i, slot) in embedded.iter_mut().enumerate() {
        *slot = le_u32(payload, 8 + i * 4, "wavetable size")? != 0;
    }
    let xml_start = 32usize;
    let xml_end = xml_start
        .checked_add(xml_size)
        .ok_or_else(|| "xml size overflow".to_string())?;
    let xml_bytes = payload
        .get(xml_start..xml_end)
        .ok_or_else(|| format!("file truncated: xml size {xml_size} exceeds payload"))?;
    // Some released patches carry non-UTF-8 bytes (e.g. in the patch name);
    // Surge reads them tolerantly, so decode lossily instead of failing.
    let xml = String::from_utf8_lossy(xml_bytes).into_owned();

    Ok(SurgePatchFile {
        name,
        xml,
        embedded_wavetables: embedded,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wrap an XML document in a valid FPCh/sub3 container, mirroring what
    /// Surge writes.
    fn wrap(xml: &str, name: &str, wtsizes: [u32; 6]) -> Vec<u8> {
        let xml_bytes = xml.as_bytes();
        let mut payload = Vec::new();
        payload.extend_from_slice(b"sub3");
        payload.extend_from_slice(&(xml_bytes.len() as u32).to_le_bytes());
        for size in wtsizes {
            payload.extend_from_slice(&size.to_le_bytes());
        }
        payload.extend_from_slice(xml_bytes);

        let mut out = Vec::new();
        out.extend_from_slice(b"CcnK");
        out.extend_from_slice(&0u32.to_be_bytes());
        out.extend_from_slice(b"FPCh");
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(b"cjs3");
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(&1u32.to_be_bytes());
        let mut name_buf = [0u8; 28];
        let name_bytes = name.as_bytes();
        let len = name_bytes.len().min(27);
        name_buf[..len].copy_from_slice(&name_bytes[..len]);
        out.extend_from_slice(&name_buf);
        out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        out.extend_from_slice(&payload);
        out
    }

    #[test]
    fn parses_container() {
        let data = wrap("<patch revision=\"9\"></patch>", "Init", [0; 6]);
        let parsed = parse_fxp(&data).expect("valid container");
        assert_eq!(parsed.name, "Init");
        assert!(parsed.xml.starts_with("<patch"));
        assert_eq!(parsed.embedded_wavetables, [false; 6]);
    }

    #[test]
    fn detects_embedded_wavetables() {
        let data = wrap("<patch/>", "WT", [0, 5, 0, 0, 0, 0]);
        let parsed = parse_fxp(&data).expect("valid container");
        assert_eq!(
            parsed.embedded_wavetables,
            [false, true, false, false, false, false]
        );
    }

    #[test]
    fn rejects_non_fxp() {
        assert!(parse_fxp(b"not an fxp at all").is_err());
    }

    #[test]
    fn rejects_wrong_fx_magic() {
        let mut data = wrap("<patch/>", "X", [0; 6]);
        data[8..12].copy_from_slice(b"FxCk");
        let err = parse_fxp(&data).unwrap_err();
        assert!(err.contains("FPCh"), "unexpected error: {err}");
    }

    #[test]
    fn accepts_raw_xml_payload() {
        let xml = "<patch revision=\"9\"><meta name=\"Raw\"/></patch>";
        let mut data = Vec::new();
        data.extend_from_slice(b"CcnK");
        data.extend_from_slice(&0u32.to_be_bytes());
        data.extend_from_slice(b"FPCh");
        data.extend_from_slice(&1u32.to_be_bytes());
        data.extend_from_slice(b"cjs3");
        data.extend_from_slice(&1u32.to_be_bytes());
        data.extend_from_slice(&1u32.to_be_bytes());
        data.extend_from_slice(&[0u8; 28]);
        data.extend_from_slice(&(xml.len() as u32).to_be_bytes());
        data.extend_from_slice(xml.as_bytes());
        let parsed = parse_fxp(&data).expect("raw xml payload");
        assert!(parsed.xml.contains("Raw"));
    }
}
