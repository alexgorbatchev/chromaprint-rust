//! Streaming AcoustID audio fingerprints, portable binary encoding, and segment matching.
//!
//! ```
//! use audio_chromaprint::{Algorithm, Context};
//! let mut context = Context::new(Algorithm::Test2);
//! context.start(11025, 1)?;
//! context.feed(&[0; 44100])?;
//! context.finish()?;
//! let encoded = context.fingerprint()?;
//! assert!(!encoded.is_empty());
//! # Ok::<(), audio_chromaprint::Error>(())
//! ```

mod codec;
mod matcher;
mod pipeline;
mod resample;

pub use codec::{
    decode, decode_base64, decode_header, decode_header_base64, encode, encode_base64,
};
pub use matcher::{Matcher, Segment};
pub use pipeline::Context;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("fingerprinting context has not been started")]
    NotStarted,
    #[error("sample rate must exceed 1000 Hz and channels must be positive")]
    InvalidAudio,
    #[error("invalid or truncated fingerprint")]
    InvalidFingerprint,
    #[error("fingerprint exceeds format or matcher limits")]
    FingerprintTooLong,
    #[error("match threshold must be finite and between zero and 32")]
    InvalidThreshold,
    #[error("invalid base64: {0}")]
    Base64(#[from] base64::DecodeError),
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Algorithm {
    Test1 = 0,
    #[default]
    Test2 = 1,
    Test3 = 2,
    Test4 = 3,
    Test5 = 4,
}

impl Algorithm {
    pub const ALL: [Self; 5] = [
        Self::Test1,
        Self::Test2,
        Self::Test3,
        Self::Test4,
        Self::Test5,
    ];
}

/// Fixed, validated settings of a Chromaprint algorithm.
#[derive(Debug, Clone, Copy)]
pub struct Config {
    pub algorithm: Algorithm,
}

impl Config {
    pub fn sample_rate(self) -> usize {
        11025
    }
    pub fn frame_size(self) -> usize {
        if self.algorithm == Algorithm::Test5 {
            2048
        } else {
            4096
        }
    }
    pub fn item_duration(self) -> usize {
        if self.algorithm == Algorithm::Test5 {
            1024
        } else {
            1365
        }
    }
    pub fn delay(self) -> usize {
        19 * self.item_duration() + self.frame_size() - self.item_duration()
    }
    pub fn item_duration_seconds(self) -> f64 {
        self.item_duration() as f64 / self.sample_rate() as f64
    }
    pub fn delay_seconds(self) -> f64 {
        self.delay() as f64 / self.sample_rate() as f64
    }
}

pub fn hamming_distance(a: u32, b: u32) -> u32 {
    (a ^ b).count_ones()
}

pub fn simhash(values: &[u32]) -> u32 {
    (0..32)
        .filter(|bit| values.iter().filter(|v| *v & (1 << bit) != 0).count() > values.len() / 2)
        .fold(0, |hash, bit| hash | (1 << bit))
}
