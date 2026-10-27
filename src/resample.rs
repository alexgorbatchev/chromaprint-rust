// Chromaprint's fixed-point polyphase sinc resampler. Its integer quantization
// is part of the fingerprint contract; general floating-point resamplers do not
// reproduce the reference PCM. Derived from gochromaprint (see LICENSE).
pub(crate) struct Resampler {
    filters: Vec<i16>,
    length: usize,
    index: i64,
    fraction: i64,
    input_rate: i64,
    output_rate: i64,
}

fn bessel(x: f64) -> f64 {
    let mut value = 1.0;
    let mut term = 1.0;
    let mut i = 1.0;
    loop {
        term *= x * x / (4.0 * i * i);
        let previous = value;
        value += term;
        if value == previous {
            return value;
        }
        i += 1.0;
    }
}

impl Resampler {
    pub(crate) fn new(input_rate: usize) -> Self {
        let factor = (11025.0 * 0.8 / input_rate as f64).min(1.0);
        let length = (16.0 / factor).ceil() as usize;
        let center = (length - 1) / 2;
        let mut filters = Vec::with_capacity(length * 256);
        for phase in 0..256 {
            let values: Vec<_> = (0..length)
                .map(|i| {
                    let x = std::f64::consts::PI
                        * (i as f64 - center as f64 - phase as f64 / 256.0)
                        * factor;
                    let sinc = if x == 0.0 { 1.0 } else { x.sin() / x };
                    let window = 2.0 * x / (factor * length as f64 * std::f64::consts::PI);
                    sinc * bessel(9.0 * (1.0 - window * window).max(0.0).sqrt())
                })
                .collect();
            let norm: f64 = values.iter().sum();
            filters.extend(
                values
                    .into_iter()
                    .map(|v| (v * 32768.0 / norm).round().clamp(-32768.0, 32767.0) as i16),
            );
        }
        Self {
            filters,
            length,
            index: -256 * center as i64,
            fraction: 0,
            input_rate: input_rate as i64,
            output_rate: 11025,
        }
    }

    pub(crate) fn process(&mut self, input: &[i16]) -> (usize, Vec<i16>) {
        if input.is_empty() {
            return (0, Vec::new());
        }
        let mut output = Vec::new();
        let increment = self.input_rate * 256;
        loop {
            let position = self.index >> 8;
            if position >= 0 && position as usize + self.length > input.len() {
                break;
            }
            let filter_start = (self.index & 255) as usize * self.length;
            // Explicit wrapping reproduces the reference's signed 32-bit accumulator.
            let mut value = 0i32;
            for (i, coefficient) in self.filters[filter_start..filter_start + self.length]
                .iter()
                .enumerate()
            {
                let source = (position + i as i64).unsigned_abs() as usize % input.len();
                value = value.wrapping_add(input[source] as i32 * *coefficient as i32);
            }
            value = value.wrapping_add(16384) >> 15;
            output.push(value.clamp(-32768, 32767) as i16);
            self.fraction += increment % self.output_rate;
            self.index += increment / self.output_rate;
            if self.fraction >= self.output_rate {
                self.fraction -= self.output_rate;
                self.index += 1;
            }
        }
        let consumed = (self.index.max(0) >> 8) as usize;
        if self.index >= 0 {
            self.index &= 255;
        }
        (consumed, output)
    }
}
