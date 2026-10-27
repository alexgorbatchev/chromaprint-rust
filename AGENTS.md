---
created_on: 2026-10-14 05:57
last_modified: 2026-10-14 05:57
status: current
---

# audio-chromaprint

This is a standalone library with no CLI entrypoint. Apply the parent Rust project instructions.

## Commands

- `just build`: build all targets.
- `just test`: run behavioral and documentation tests.
- `just lint`: check formatting and Clippy with warnings denied.
- `just coverage`: require at least 90% line coverage with cargo-llvm-cov and llvm-tools.
- `just check`: run lint, tests, and coverage.

## Contracts

- Preserve the native compressed format and URL-safe unpadded base64. Do not trust encoded lengths before validating the available payload.
- Preserve all five algorithms, fixed-point resampling, streaming across partial multichannel frames, and continued pipeline state after `clear`.
- Use RustFFT directly for transforms. The maintained rusty-chromaprint compressor is used directly; its incomplete fingerprint presets are unsuitable for the full pipeline.
- Keep audio history bounded. Fingerprint accumulation may grow until the caller clears it.
- Always divide wide multichannel sums before narrowing. Merge segments only when adjacent in both recordings.
- Use behavioral golden fixtures, red/green development, a disabled-implementation regression check, and at least 90% line coverage. Any runtime code change requires a corresponding test-file change.
- Keep evidence in `.tmp/`. Keep standing policy here, updating it in place; keep assignments, progress, and test-run history outside this file.
- Retain algorithm and fixture copyright notices. Never publish packages, releases, or tags without explicit user authorization.
