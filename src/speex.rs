//! Speex NB decode via FFI using speex-sys.

use anyhow::anyhow;
use speex_sys as ffi;
use std::os::raw::{c_int, c_void};

use audio_codec::Decoder as _;

const SPEEX_SET_SAMPLING_RATE: c_int = 24;
const SPEEX_GET_FRAME_SIZE: c_int = 3;

pub struct FritzSpeexDecoder {
    state: *mut c_void,
    frame_size: usize,
}

// SAFETY: Send is sound because each SpeexDecoder uniquely owns one heap-
// allocated speex decoder state, so moving the struct to another thread
// moves the only reference to that state along with it. speex keeps no
// thread-local or thread-affine data (no TLS slots, no thread handles) in the
// decoder, so the state remains usable after a move.
unsafe impl Send for FritzSpeexDecoder {}
// SAFETY: Sync is sound because every access to the raw decoder state goes
// through `&mut self` (`decode_into`, Drop). The borrow checker therefore
// prevents two threads from touching the state concurrently, and since the
// state holds no thread-local data either, sharing `&SpeexDecoder` cannot
// cause a data race or a cross-thread state mix-up.
unsafe impl Sync for FritzSpeexDecoder {}

impl FritzSpeexDecoder {
    /// # Errors
    ///
    /// Returns an error if speex cannot allocate a decoder state.
    pub fn new() -> anyhow::Result<Self> {
        use ffi::{SpeexMode, speex_decoder_ctl, speex_decoder_init, speex_nb_mode};
        unsafe {
            let mode: *const SpeexMode = std::ptr::addr_of!(speex_nb_mode);
            let state = speex_decoder_init(mode);
            if state.is_null() {
                return Err(anyhow!("speex_decoder_init failed"));
            }
            let mut rate: c_int = 8000;
            speex_decoder_ctl(
                state,
                SPEEX_SET_SAMPLING_RATE,
                std::ptr::addr_of_mut!(rate).cast(),
            );
            let mut fs: c_int = 0;
            speex_decoder_ctl(
                state,
                SPEEX_GET_FRAME_SIZE,
                std::ptr::addr_of_mut!(fs).cast(),
            );
            Ok(Self {
                state,
                frame_size: fs.max(160).try_into().unwrap_or(160),
            })
        }
    }
}

impl Drop for FritzSpeexDecoder {
    fn drop(&mut self) {
        use ffi::speex_decoder_destroy;
        unsafe {
            speex_decoder_destroy(self.state);
        }
    }
}

impl audio_codec::Decoder for FritzSpeexDecoder {
    fn decode_into(
        &mut self,
        data: &[u8],
        out: &mut [audio_codec::Sample],
    ) -> Result<usize, audio_codec::CodecError> {
        use ffi::{
            SpeexBits, speex_bits_destroy, speex_bits_init, speex_bits_read_from, speex_decode_int,
        };
        if out.len() < self.frame_size {
            return Err(audio_codec::CodecError::BufferTooSmall);
        }
        unsafe {
            let mut bits = SpeexBits {
                chars: std::ptr::null_mut(),
                nbBits: 0,
                charPtr: 0,
                bitPtr: 0,
                owner: 0,
                overflow: 0,
                buf_size: 0,
                reserved1: 0,
                reserved2: std::ptr::null_mut(),
            };
            let bits_ptr = std::ptr::addr_of_mut!(bits);
            speex_bits_init(bits_ptr);
            let len: c_int = data
                .len()
                .try_into()
                .map_err(|_| audio_codec::CodecError::InvalidInput)?;
            speex_bits_read_from(bits_ptr, data.as_ptr().cast_mut().cast(), len);
            let ret = speex_decode_int(self.state, bits_ptr, out.as_mut_ptr());
            speex_bits_destroy(bits_ptr);
            match ret {
                0 => Ok(self.frame_size),
                _ => Err(audio_codec::CodecError::DecodeFailed),
            }
        }
    }

    fn max_decode_samples(&self, n_bytes: usize) -> usize {
        n_bytes.div_ceil(RAW38_FRAME_LEN) * self.frame_size
    }

    fn sample_rate(&self) -> u32 {
        8000
    }

    fn channels(&self) -> u16 {
        1
    }
}

/// Decodes a raw Speex stream of consecutive 38-byte frames (TTS files).
pub struct Raw38Decoder {
    inner: FritzSpeexDecoder,
}

impl Raw38Decoder {
    /// # Errors
    ///
    /// Returns an error if speex cannot allocate a decoder state.
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            inner: FritzSpeexDecoder::new()?,
        })
    }

    #[must_use]
    pub fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    /// # Errors
    ///
    /// Returns an error if the data length is not a multiple of 38 or if
    /// speex reports a decode failure.
    pub fn decode(&mut self, data: &[u8]) -> anyhow::Result<Vec<i16>> {
        let mut out = Vec::with_capacity(data.len() / RAW38_FRAME_LEN * self.inner.frame_size);
        let mut frame = vec![0i16; self.inner.frame_size];
        for chunk in data.as_chunks::<RAW38_FRAME_LEN>().0 {
            let n = self
                .inner
                .decode_into(chunk, &mut frame)
                .map_err(|e| anyhow!("speex decode: {e}"))?;
            out.extend_from_slice(&frame[..n]);
        }
        Ok(out)
    }
}

pub const RAW38_FRAME_LEN: usize = 38;
