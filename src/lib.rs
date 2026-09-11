pub mod container;
pub mod speex;
mod wav;

use anyhow::anyhow;
use std::path::Path;

use audio_codec::g722::G722Decoder;
use container::Frame;
use speex::FritzSpeexDecoder;
use stdio_override::StderrOverride;
use tempfile::NamedTempFile;

/// PCM samples produced per codec frame. Both Speex NB (38 bytes @ 8 kHz,
/// 20 ms) and G.722 (80 bytes @ 16 kHz, 10 ms) output 160 samples per frame.
const FRAME_SAMPLES: usize = 160;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    SpeexNb,
    G722,
    Raw38,
}

#[derive(Debug)]
pub struct DecodeResult {
    pub sample_rate: u32,
    pub samples: Vec<i16>,
    pub audio_frames: usize,
    pub silence_frames: usize,
}

impl DecodeResult {
    #[must_use]
    pub const fn total_duration(&self) -> std::time::Duration {
        std::time::Duration::from_micros(
            self.samples.len() as u64 * 1_000_000 / self.sample_rate as u64,
        )
    }
}

fn decode_frames<D: audio_codec::Decoder>(mut dec: D, frames: &[Frame]) -> DecodeResult {
    let (mut audio_frames, mut silence_frames, mut decode_errors) = (0usize, 0usize, 0usize);
    let mut samples = Vec::with_capacity(frames.len() * FRAME_SAMPLES);

    for f in frames {
        match f {
            Frame::Audio(p) => {
                audio_frames += 1;
                if let Err(e) = decode_payload(&mut dec, p, &mut samples) {
                    decode_errors += 1;
                    log::debug!("decode error on audio frame {audio_frames}: {e}");
                    samples.resize(samples.len() + FRAME_SAMPLES, 0);
                }
            }
            Frame::Silence => {
                silence_frames += 1;
                samples.resize(samples.len() + FRAME_SAMPLES, 0);
            }
            Frame::End => break,
        }
    }

    if decode_errors > 0 {
        log::warn!("{decode_errors} frames failed to decode");
    }

    DecodeResult {
        sample_rate: dec.sample_rate(),
        samples,
        audio_frames,
        silence_frames,
    }
}

fn decode_payload<D: audio_codec::Decoder>(
    dec: &mut D,
    payload: &[u8],
    out: &mut Vec<i16>,
) -> anyhow::Result<()> {
    let mut buf = vec![0i16; dec.max_decode_samples(payload.len())];
    let n = dec.decode_into(payload, &mut buf)?;
    out.extend_from_slice(&buf[..n]);
    Ok(())
}

fn wrap_stderr<R, F: FnOnce() -> R>(closure: F) -> anyhow::Result<(R, String)> {
    let tmpfile = NamedTempFile::new()?;
    let stderr = StderrOverride::from_file(tmpfile.as_ref())?;
    let res = closure();
    stderr.reset()?;
    Ok((res, std::fs::read_to_string(tmpfile)?))
}
fn decode(data: &[u8], codec: Option<Codec>) -> anyhow::Result<DecodeResult> {
    if codec == Some(Codec::Raw38) {
        return decode_raw38(data);
    }
    match container::detect_stream_kind(data) {
        container::StreamKind::Raw38 => decode_raw38(data),
        container::StreamKind::Container => {
            log::debug!("detected container stream!");
            let frames = container::parse_container(data)?;
            log::debug!("parsed {} frames", frames.len());
            let codec = match codec {
                Some(codec) => codec,
                None => detect_codec(&frames)?,
            };
            log::debug!("using codec: {codec:?}");
            match codec {
                Codec::SpeexNb => {
                    let decoder = FritzSpeexDecoder::new()?;
                    let (res, stderr) = wrap_stderr(|| decode_frames(decoder, &frames))?;
                    if !stderr.is_empty() {
                        log::debug!("speex stderr while decoding:\n{stderr}");
                    }
                    Ok(res)
                }
                Codec::G722 => Ok(decode_frames(G722Decoder::default(), &frames)),
                Codec::Raw38 => unreachable!(),
            }
        }
    }
}

/// Reads `input`, decodes it (auto-detecting when `codec` is `None`) and
/// writes the PCM as a WAV to `output`.
///
/// # Errors
///
/// Returns an error if the input cannot be read, decoding fails, or the
/// output cannot be written.
pub fn decode_to_wav(
    input: &Path,
    output: &Path,
    codec: Option<Codec>,
) -> anyhow::Result<DecodeResult> {
    let data = std::fs::read(input).map_err(|e| anyhow!("read {}: {e}", input.display()))?;
    let res = decode(&data, codec)?;
    wav::write_wav(output, res.sample_rate, &res.samples)
        .map_err(|e| anyhow!("write {}: {e}", output.display()))?;
    Ok(res)
}

fn detect_codec(frames: &[Frame]) -> anyhow::Result<Codec> {
    let audio: Vec<&[u8]> = frames
        .iter()
        .filter_map(|f| match f {
            Frame::Audio(p) if !p.is_empty() => Some(p.as_slice()),
            _ => None,
        })
        .take(64)
        .collect();

    let (speex_failed, stderr) = wrap_stderr(|| {
        FritzSpeexDecoder::new().map_or(usize::MAX, |mut d| probe_failures(&audio, &mut d))
    })?;
    if !stderr.is_empty() {
        log::trace!("speex stderr while probing:\n{stderr}");
    }
    let g722_failed = probe_failures(&audio, &mut G722Decoder::default());

    match (speex_failed, g722_failed) {
        (0, 0) => Err(anyhow!(
            "codec detection ambiguous (both codecs decoded every frame); specify it with --codec"
        )),
        (_, 0) => Ok(Codec::G722),
        (0, _) => Ok(Codec::SpeexNb),
        _ => Err(anyhow!(
            "codec detection failed (both codecs dropped {speex_failed} and {g722_failed} frames); specify the codec with --codec"
        )),
    }
}

fn probe_failures(frames: &[&[u8]], dec: &mut impl audio_codec::Decoder) -> usize {
    frames
        .iter()
        .filter(|p| {
            let mut buf = vec![0i16; dec.max_decode_samples(p.len())];
            dec.decode_into(p, &mut buf).is_err()
        })
        .count()
}

fn decode_raw38(data: &[u8]) -> anyhow::Result<DecodeResult> {
    let mut decoder = speex::Raw38Decoder::new()?;
    let sample_rate = decoder.sample_rate();
    let (res, stderr) = wrap_stderr(|| decoder.decode(data))?;
    if !stderr.is_empty() {
        log::debug!("raw38 stderr while decoding:\n{stderr}");
    }
    let samples = res?;
    let frames = samples.len() / FRAME_SAMPLES;
    Ok(DecodeResult {
        sample_rate,
        samples,
        audio_frames: frames,
        silence_frames: 0,
    })
}
