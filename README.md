# audio-chromaprint

Streaming AcoustID audio fingerprints and segment matching in pure Rust. This library replaces [gochromaprint](https://github.com/alexgorbatchev/gochromaprint).

## What It Does

- **Audio fingerprints:** Process interleaved signed 16-bit PCM with all five Chromaprint algorithms, downmixing, resampling, interpolation, and optional leading-silence removal.
- **Portable fingerprints:** Read and write Chromaprint binary and URL-safe, unpadded base64 fingerprints, including arbitrary algorithm tags.
- **Audio comparison:** Find aligned matching segments with configurable error thresholds and calculate SimHash and Hamming distance.
- **Streaming:** Feed arbitrary chunk boundaries, clear accumulated fingerprint items while retaining audio state, or restart for a new recording.

## How It Works

The processor downmixes PCM, resamples it to 11025 Hz, computes overlapping spectra, extracts twelve pitch classes, filters and normalizes them, and quantizes rectangular image features into 32-bit subfingerprints.

## How it Really Works

[RustFFT](https://docs.rs/rustfft/) performs forward transforms through its planner and reusable scratch buffers. The [rusty-chromaprint compressor](https://docs.rs/rusty-chromaprint/latest/rusty_chromaprint/struct.FingerprintCompressor.html) writes the native three/five-bit format; [base64](https://docs.rs/base64/) handles the alphabet and padding rules. The fixed-point sinc resampler and algorithm coefficients preserve reference fingerprints. Audio history stays bounded; accumulated raw fingerprint items grow until cleared.

The complete fingerprinting pipeline is implemented here because rusty-chromaprint 0.3.0 ignores interpolation and silence-removal settings, and its presets four and five omit required configuration. The native C binding adds a native build and bundled LGPL components. Neither meets this library's complete pure Rust contract. The compressor is used directly through its public API.

## Installation

From a sibling project:

```sh
cargo add audio-chromaprint --path ../chromaprint
```

## Quick Start

```rust
use audio_chromaprint::{Algorithm, Context};

let mut context = Context::new(Algorithm::Test2);
context.start(44100, 2)?;
let samples = vec![0i16; 44100 * 2]; // Replace with interleaved stereo PCM.
context.feed(&samples)?;
context.finish()?;
let encoded = context.fingerprint()?;
# Ok::<(), audio_chromaprint::Error>(())
```

## API

`Context` exposes `start`, `feed`, `finish`, `raw_fingerprint`, `fingerprint`, `fingerprint_hash`, `clear`, `algorithm`, `config`, and `set_silence_threshold`. `Config` provides frame size, sample rate, item duration, and algorithm delay in samples or seconds. `Algorithm::Test2` is the default; `Algorithm::ALL` enumerates the five supported algorithms. `finish` flushes complete audio frames and discards incomplete FFT windows; an incomplete interleaved channel frame remains available for a later feed.

`encode` and `decode` operate on binary bytes; `encode_base64` and `decode_base64` use strings. Decoding returns `(raw_items, algorithm_tag)`. Header readers return `(item_count, algorithm_tag)` without unpacking items. `Matcher::new(algorithm)` provides `match_fingerprints`, `set_match_threshold`, `match_threshold`, `hash_time`, and `hash_duration`. Segments expose offsets, duration, mean bit errors, public score, similarity percentage, and checked adjacent merging.

Behavior corrections from the Go implementation: stereo sums are divided before narrowing to 16 bits, preventing loud-signal corruption; matching segments must be adjacent in both fingerprints before merging, preventing matches from bridging rejected regions. Unsupported algorithms are unrepresentable through the typed enum. Invalid audio, thresholds, oversized encodings, malformed base64, and corrupt binary payloads return typed errors. Decoder allocation is bounded by the supplied payload rather than trusting its length header.

Golden tests include the original Go fixtures and outputs produced by native fpcalc 1.6.1. Algorithm three's oracle is the original Go implementation because the installed native fpcalc returned an empty fingerprint for that preset.

## License

MIT. Original algorithm and fixture attribution: Alex Gorbatchev and Lukas Lalinsky; see [LICENSE](LICENSE). Dependency licenses are retained by Cargo packages.
