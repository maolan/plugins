use std::{fs::File, path::Path};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

#[derive(Debug, Clone)]
pub struct AudioFile {
    pub path: String,
    pub sample_rate: u32,
    pub data: Vec<f32>,
    pub channels: u16,
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("audio decode error: {0}")]
    Decode(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<symphonia::core::errors::Error> for LoadError {
    fn from(err: symphonia::core::errors::Error) -> Self {
        LoadError::Decode(err.to_string())
    }
}

impl AudioFile {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, LoadError> {
        let path = path.as_ref();
        let file = File::open(path)?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        let format_opts = FormatOptions::default();
        let metadata_opts = MetadataOptions::default();
        let decoder_opts = DecoderOptions::default();

        let probed = symphonia::default::get_probe().format(&hint, mss, &format_opts, &metadata_opts)?;
        let mut format = probed.format;

        let track = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
            .or_else(|| format.tracks().first())
            .ok_or_else(|| LoadError::Decode("no usable audio track".into()))?;

        let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(1) as u16;
        let sample_rate = track.codec_params.sample_rate.unwrap_or(48_000);
        let track_id = track.id;

        let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &decoder_opts)?;

        let mut sample_buf = None;
        let mut data = Vec::new();

        loop {
            let packet = match format.next_packet() {
                Ok(packet) => packet,
                Err(SymphoniaError::IoError(e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    break;
                }
                Err(e) => return Err(e.into()),
            };

            if packet.track_id() != track_id {
                continue;
            }

            let decoded = decoder.decode(&packet)?;

            if sample_buf.is_none() {
                let spec = *decoded.spec();
                sample_buf = Some(SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
            }
            let buf = sample_buf.as_mut().unwrap();
            buf.copy_planar_ref(decoded);
            data.extend_from_slice(buf.samples());
        }

        Ok(Self {
            path: path.display().to_string(),
            sample_rate,
            data,
            channels,
        })
    }


    pub fn frame(&self, pos: usize) -> &[f32] {
        let start = pos * self.channels as usize;
        let end = start + self.channels as usize;
        &self.data[start.min(self.data.len())..end.min(self.data.len())]
    }

    pub fn num_frames(&self) -> usize {
        self.data.len() / self.channels as usize
    }
}
