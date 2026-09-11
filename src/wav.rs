//! WAV output via the `hound` crate.

use std::path::Path;

/// # Errors
///
/// The file must be created and written to.
pub fn write_wav(path: &Path, sample_rate: u32, samples: &[i16]) -> anyhow::Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for &s in samples {
        writer.write_sample(s)?;
    }
    writer.finalize()?;
    Ok(())
}
