//! TAM container format parser.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    /// Speech/audio frame with its payload (G.722 or Speex bytes).
    Audio(Vec<u8>),
    /// Silence/CNG frame. Carries no codec payload.
    Silence,
    /// End-of-stream or terminator marker.
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndCode {
    /// `0x00` end-of-stream
    Eos,
    /// `0xFC` / `0xFD` / `0xFE` touch terminator
    Terminator(u8),
}

#[derive(Debug)]
pub struct ParseError {
    pub offset: usize,
    pub reason: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "offset {}: {}", self.offset, self.reason)
    }
}

impl std::error::Error for ParseError {}

/// Parse the container into an ordered frame list.
///
/// # Errors
///
/// [`ParseError`] if a frame length overruns the input.
pub fn parse_container(data: &[u8]) -> Result<Vec<Frame>, ParseError> {
    let mut frames = Vec::new();
    let mut pos = 0usize;
    loop {
        if pos >= data.len() {
            break;
        }
        let b = data[pos];
        match b {
            0x00 | 0xFC..=0xFE => {
                frames.push(Frame::End);
                break;
            }
            0xFB => {
                frames.push(Frame::Silence);
                pos += 2;
            }
            0xFF => {
                if pos + 2 >= data.len() {
                    return Err(ParseError {
                        offset: pos,
                        reason: "truncated extended length".into(),
                    });
                }
                let len = u16::from_le_bytes([data[pos + 1], data[pos + 2]]) as usize;
                pos += 3;
                if pos + len > data.len() {
                    return Err(ParseError {
                        offset: pos,
                        reason: format!("extended frame of {len} bytes overruns input"),
                    });
                }
                frames.push(Frame::Audio(data[pos..pos + len].to_vec()));
                pos += len;
            }
            0x01..=0xFA => {
                let len = b as usize;
                if pos + 1 + len > data.len() {
                    return Err(ParseError {
                        offset: pos,
                        reason: format!("frame of {len} bytes overruns input"),
                    });
                }
                frames.push(Frame::Audio(data[pos + 1..pos + 1 + len].to_vec()));
                pos += 1 + len;
            }
        }
    }
    Ok(frames)
}

/// Walk the container chain from `start`.
///
/// Returns `true` when the markers form a valid sequence reaching exactly
/// `data.len()`, i.e. the whole file is a container. Distinguishes a
/// length-prefixed container from a raw 38-byte Speex stream during format
/// auto-detection.
#[must_use]
pub const fn is_container_stream(data: &[u8], start: usize) -> bool {
    let mut pos = start;
    while pos < data.len() {
        let b = data[pos];
        match b {
            0x00 | 0xFC..=0xFE => {
                pos += 1;
                break;
            }
            0xFB => pos += 2,
            0xFF => {
                if pos + 2 >= data.len() {
                    return false;
                }
                let len = u16::from_le_bytes([data[pos + 1], data[pos + 2]]) as usize;
                pos += 3 + len;
            }
            0x01..=0xFA => pos += 1 + b as usize,
        }
    }
    pos == data.len()
}

/// Try to detect whether `data` is a raw 38-byte Speex stream (TTS/fvp files)
/// or a length-prefixed container (rec files).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
    Container,
    Raw38,
}

#[must_use]
pub fn detect_stream_kind(data: &[u8]) -> StreamKind {
    if data.is_empty() {
        return StreamKind::Raw38;
    }
    let b0 = data[0];
    if b0 == 0 || b0 == 0xFB || b0 == 0xFF || (0xFC..=0xFE).contains(&b0) {
        // Starts with a container marker -> container.
        return StreamKind::Container;
    }
    if b0 > 0xFA {
        return StreamKind::Raw38;
    }
    if is_container_stream(data, 0) {
        StreamKind::Container
    } else {
        StreamKind::Raw38
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_and_audio() {
        let data = [
            0x03, b'a', b'b', b'c', // audio frame, len 3
            0xFB, 0x12, // silence + ctrl
            0x02, b'd', b'e', // audio frame, len 2
            0x00, // end
        ];
        let frames = parse_container(&data).unwrap();
        assert_eq!(frames.len(), 4);
        match &frames[0] {
            Frame::Audio(p) => assert_eq!(p, b"abc"),
            _ => panic!(),
        }
        assert_eq!(frames[1], Frame::Silence);
        match &frames[2] {
            Frame::Audio(p) => assert_eq!(p, b"de"),
            _ => panic!(),
        }
        assert_eq!(frames[3], Frame::End);
    }

    #[test]
    fn extended_length() {
        let data = [0xFF, 0x05, 0x00, 1, 2, 3, 4, 5, 0x00];
        let frames = parse_container(&data).unwrap();
        match &frames[0] {
            Frame::Audio(p) => assert_eq!(p, &[1, 2, 3, 4, 5]),
            _ => panic!(),
        }
    }

    #[test]
    fn chain_detection() {
        let container = [0x03, 1, 2, 3, 0x05, 4, 5, 6, 7, 8, 0x00];
        assert_eq!(detect_stream_kind(&container), StreamKind::Container);
        // Raw 38-byte stream with a plausible header byte.
        let mut raw = vec![0u8; 38];
        raw[0] = 0x26;
        let mut raw2 = raw.clone();
        raw2.extend_from_slice(&raw);
        assert_eq!(detect_stream_kind(&raw2), StreamKind::Raw38);
    }
}
