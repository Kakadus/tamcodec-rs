# Fritz!Box Telephone Answering Machine decoder

Decode Fritz!Box TAM recordings into WAV.

```
tamcodec rec.X.YYY.Z out.wav            # auto-detect codec
tamcodec rec.X.YYY.Z out.wav -c speex   # force Speex NB
tamcodec rec.X.YYY.Z out.wav -c g722    # force G.722
tamcodec tts.X.YYY.Z out.wav -c raw38   # force raw Speex (TTS files)
```

## Container Reference

Container markers (0x01–0xFA are length-prefixed audio frames):

| Byte                              | Meaning                                 |
|-----------------------------------|-----------------------------------------|
| `0x00`                            | End-of-stream                           |
| `FB xx`                           | Silence / comfort-noise frame (2 B)     |
| `FF` + 2-byte LE length + payload | Extended-length audio frame (len > 250) |
| `FC` / `FD` / `FE`                | End-of-stream (terminator)              |

See `container.rs` for more details.

## Codecs

Newer Fritz!Box OS uses Speex or G.722 for encoding voice messages. However, the codec information is not stored in the
file, but in the accompanying metadata (e.g. `meta0`) file. So, decoding the audio file only may be ambigous and
requires you to specify the codec explicitly in such cases. If you encounter any problems, please open an issue and
include output of `RUST_LOG=trace`.

## TTS files

The TTS files contain raw speex without any container format.

## Build

This uses speex-sys to statically link against Speex.

```
cargo build --release
```

You can download binaries from the build artifact of each commit.

## References

Thanks to https://github.com/msilvoso/speexdec-fb, which laid the groundwork for Speex decoding. Historic context is
available at https://www.ip-phone-forum.de/threads/fritz-box-anrufbeantworter-encoder-decoder.156186/.
