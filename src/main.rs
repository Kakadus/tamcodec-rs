use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use tamcodec::Codec;

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
enum CodecArg {
    #[default]
    Auto,
    Speex,
    G722,
    Raw38,
}

impl CodecArg {
    const fn option(self) -> Option<Codec> {
        match self {
            Self::Auto => None,
            Self::Speex => Some(Codec::SpeexNb),
            Self::G722 => Some(Codec::G722),
            Self::Raw38 => Some(Codec::Raw38),
        }
    }
}

#[derive(Parser)]
#[command(about = "Decode a Fritz!Box TAM recording to WAV")]
struct Args {
    input: PathBuf,
    output: PathBuf,
    #[arg(short, long, value_enum, default_value_t)]
    codec: CodecArg,
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let args = Args::parse();

    let res = tamcodec::decode_to_wav(&args.input, &args.output, args.codec.option())?;

    println!(
        "{} -> {}\n sample rate: {} Hz\n  {} audio frames, {} silence frames\n  {} samples ({:.2} s)",
        args.input.display(),
        args.output.display(),
        res.sample_rate,
        res.audio_frames,
        res.silence_frames,
        res.samples.len(),
        res.total_duration().as_secs_f32(),
    );
    Ok(())
}
