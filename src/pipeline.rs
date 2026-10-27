use crate::{Algorithm, Config, Error, encode_base64, resample::Resampler, simhash};
use rustfft::{Fft, FftPlanner, num_complex::Complex};
use std::{collections::VecDeque, sync::Arc};

/// A reusable stream processor. Audio state is bounded independently of stream
/// length; only the returned fingerprint grows. `clear` releases those items.
pub struct Context {
    config: Config,
    channels: usize,
    input: Vec<i16>,
    mono: Vec<i16>,
    resampler: Option<Resampler>,
    samples: VecDeque<i16>,
    chroma: VecDeque<[f64; 12]>,
    image: VecDeque<[f64; 13]>,
    fft: Arc<dyn Fft<f64>>,
    spectrum: Vec<Complex<f64>>,
    scratch: Vec<Complex<f64>>,
    window: Vec<f64>,
    notes: Vec<(usize, f64)>,
    raw: Vec<u32>,
    threshold: i32,
    silence: bool,
    average: VecDeque<i32>,
    average_sum: i32,
}

impl Context {
    pub fn new(algorithm: Algorithm) -> Self {
        let config = Config { algorithm };
        let n = config.frame_size();
        let fft = FftPlanner::new().plan_fft_forward(n);
        let scratch = vec![Complex::default(); fft.get_inplace_scratch_len()];
        let window = (0..n)
            .map(|i| {
                (0.54 - 0.46 * (i as f64 * 2.0 * std::f64::consts::PI / (n - 1) as f64).cos())
                    / 32767.0
            })
            .collect();
        let notes = (0..n / 2 + 1)
            .map(|i| {
                let octave = ((i.max(1) as f64 * 11025.0 / n as f64) / 27.5).log2();
                let note = 12.0 * (octave - octave.floor());
                (note.floor() as usize, note.fract())
            })
            .collect();
        Self {
            config,
            channels: 0,
            input: Vec::new(),
            mono: Vec::new(),
            resampler: None,
            samples: VecDeque::new(),
            chroma: VecDeque::new(),
            image: VecDeque::from([[0.0; 13]]),
            fft,
            spectrum: vec![Complex::default(); n],
            scratch,
            window,
            notes,
            raw: Vec::new(),
            threshold: 50,
            silence: algorithm == Algorithm::Test4,
            average: VecDeque::new(),
            average_sum: 0,
        }
    }

    pub fn algorithm(&self) -> Algorithm {
        self.config.algorithm
    }
    pub fn config(&self) -> Config {
        self.config
    }
    pub fn set_silence_threshold(&mut self, value: i32) {
        self.threshold = value;
    }

    /// Reset the complete pipeline for interleaved signed 16-bit PCM.
    pub fn start(&mut self, sample_rate: usize, channels: usize) -> Result<(), Error> {
        if sample_rate <= 1000 || channels == 0 {
            return Err(Error::InvalidAudio);
        }
        self.channels = channels;
        self.input.clear();
        self.mono.clear();
        self.samples.clear();
        self.chroma.clear();
        self.image.clear();
        self.image.push_back([0.0; 13]);
        self.raw.clear();
        self.resampler = (sample_rate != 11025).then(|| Resampler::new(sample_rate));
        self.silence = self.algorithm() == Algorithm::Test4;
        self.average.clear();
        self.average_sum = 0;
        Ok(())
    }

    /// Chunks may end in the middle of a multichannel frame; its samples carry
    /// over to the next call. An incomplete final frame is discarded.
    pub fn feed(&mut self, input: &[i16]) -> Result<(), Error> {
        if self.channels == 0 {
            return Err(Error::NotStarted);
        }
        // Process bounded chunks even when the caller passes an entire file.
        for chunk in input.chunks(32768) {
            self.input.extend_from_slice(chunk);
            let used = self.input.len() / self.channels * self.channels;
            let mono: Vec<_> = self.input[..used]
                .chunks_exact(self.channels)
                .map(|frame| {
                    (frame.iter().map(|v| *v as i64).sum::<i64>() / self.channels as i64) as i16
                })
                .collect();
            self.input.drain(..used);
            for value in mono {
                self.mono.push(value);
                if self.mono.len() == 32768 {
                    self.flush_audio();
                }
            }
        }
        Ok(())
    }

    pub fn finish(&mut self) -> Result<(), Error> {
        if self.channels == 0 {
            return Err(Error::NotStarted);
        }
        self.flush_audio();
        Ok(())
    }

    fn flush_audio(&mut self) {
        let output = if let Some(resampler) = &mut self.resampler {
            let (used, output) = resampler.process(&self.mono);
            self.mono.drain(..used);
            output
        } else {
            std::mem::take(&mut self.mono)
        };
        for sample in output {
            if self.silence {
                let value = (sample as i32).abs();
                self.average_sum += value;
                self.average.push_back(value);
                if self.average.len() > 55 {
                    self.average_sum -= self.average.pop_front().unwrap();
                }
                if self.average_sum / self.average.len() as i32 <= self.threshold {
                    continue;
                }
                self.silence = false;
            }
            self.samples.push_back(sample);
            if self.samples.len() == self.config.frame_size() {
                self.frame();
                self.samples.drain(..self.config.item_duration());
            }
        }
    }

    fn frame(&mut self) {
        for (i, sample) in self.samples.iter().enumerate() {
            self.spectrum[i] = Complex::new(*sample as f64 * self.window[i], 0.0);
        }
        self.fft
            .process_with_scratch(&mut self.spectrum, &mut self.scratch);
        let n = self.config.frame_size() as f64;
        let min = (n * 28.0 / 11025.0).round().max(1.0) as usize;
        let max = (n * 3520.0 / 11025.0).round().min(n / 2.0) as usize;
        let mut row = [0.0; 12];
        for i in min..max {
            let power = self.spectrum[i].norm_sqr();
            let (note, fraction) = self.notes[i];
            if self.algorithm() == Algorithm::Test3 {
                let (other, weight) = if fraction < 0.5 {
                    ((note + 11) % 12, 0.5 + fraction)
                } else {
                    ((note + 1) % 12, 1.5 - fraction)
                };
                row[note] += power * weight;
                row[other] += power * (1.0 - weight);
            } else {
                row[note] += power;
            }
        }
        self.chroma.push_back(row);
        if self.chroma.len() < 5 {
            return;
        }
        let mut filtered = [0.0; 12];
        for (row, weight) in self.chroma.iter().zip([0.25, 0.75, 1.0, 0.75, 0.25]) {
            for (value, input) in filtered.iter_mut().zip(row) {
                *value += input * weight;
            }
        }
        self.chroma.pop_front();
        let norm = filtered.iter().map(|v| v * v).sum::<f64>().sqrt();
        let previous = self.image.back().unwrap();
        let mut integral = [0.0; 13];
        let mut sum = 0.0;
        for i in 0..12 {
            sum += if norm < 0.01 { 0.0 } else { filtered[i] / norm };
            integral[i + 1] = sum + previous[i + 1];
        }
        self.image.push_back(integral);
        if self.image.len() > 257 {
            self.image.pop_front();
        }
        if self.image.len() > 16 {
            let start = self.image.len() - 17;
            let table = if self.algorithm() == Algorithm::Test1 {
                &TEST1
            } else {
                &TEST2
            };
            let value = table.iter().fold(0, |raw, classifier| {
                let response = classifier.response(&self.image, start);
                let quantized = classifier
                    .thresholds
                    .iter()
                    .filter(|t| response >= **t)
                    .count();
                (raw << 2) | [0, 1, 3, 2][quantized]
            });
            self.raw.push(value);
        }
    }

    pub fn raw_fingerprint(&self) -> &[u32] {
        &self.raw
    }
    pub fn fingerprint(&self) -> Result<String, Error> {
        encode_base64(&self.raw, self.algorithm() as u8)
    }
    pub fn fingerprint_hash(&self) -> u32 {
        simhash(&self.raw)
    }
    pub fn clear(&mut self) {
        self.raw.clear();
    }
}

struct Classifier {
    kind: usize,
    y: usize,
    height: usize,
    width: usize,
    thresholds: [f64; 3],
}
impl Classifier {
    const fn new(kind: usize, y: usize, height: usize, width: usize, thresholds: [f64; 3]) -> Self {
        Self {
            kind,
            y,
            height,
            width,
            thresholds,
        }
    }
    fn response(&self, image: &VecDeque<[f64; 13]>, x: usize) -> f64 {
        let Self {
            kind,
            y,
            height: h,
            width: w,
            ..
        } = *self;
        let area = |x1: usize, y1: usize, x2: usize, y2: usize| {
            image[x2][y2] - image[x2][y1] - image[x1][y2] + image[x1][y1]
        };
        let (a, b) = match kind {
            0 => (area(x, y, x + w, y + h), 0.0),
            1 => (
                area(x, y + h / 2, x + w, y + h),
                area(x, y, x + w, y + h / 2),
            ),
            2 => (
                area(x + w / 2, y, x + w, y + h),
                area(x, y, x + w / 2, y + h),
            ),
            3 => (
                area(x, y + h / 2, x + w / 2, y + h) + area(x + w / 2, y, x + w, y + h / 2),
                area(x, y, x + w / 2, y + h / 2) + area(x + w / 2, y + h / 2, x + w, y + h),
            ),
            4 => (
                area(x, y + h / 3, x + w, y + 2 * (h / 3)),
                area(x, y, x + w, y + h / 3) + area(x, y + 2 * (h / 3), x + w, y + h),
            ),
            _ => (
                area(x + w / 3, y, x + 2 * (w / 3), y + h),
                area(x, y, x + w / 3, y + h) + area(x + 2 * (w / 3), y, x + w, y + h),
            ),
        };
        ((1.0 + a) / (1.0 + b)).ln()
    }
}

const TEST1: [Classifier; 16] = [
    Classifier::new(0, 0, 3, 15, [2.10543, 2.45354, 2.69414]),
    Classifier::new(1, 0, 4, 14, [-0.345922, 0.0463746, 0.446251]),
    Classifier::new(1, 4, 4, 11, [-0.392132, 0.0291077, 0.443391]),
    Classifier::new(3, 0, 4, 14, [-0.192851, 0.00583535, 0.204053]),
    Classifier::new(2, 8, 2, 4, [-0.0771619, -0.00991999, 0.0575406]),
    Classifier::new(5, 6, 2, 15, [-0.710437, -0.518954, -0.330402]),
    Classifier::new(1, 9, 2, 16, [-0.353724, -0.0189719, 0.289768]),
    Classifier::new(3, 4, 2, 10, [-0.128418, -0.0285697, 0.0591791]),
    Classifier::new(3, 9, 2, 16, [-0.139052, -0.0228468, 0.0879723]),
    Classifier::new(2, 1, 3, 6, [-0.133562, 0.00669205, 0.155012]),
    Classifier::new(3, 3, 6, 2, [-0.0267, 0.00804829, 0.0459773]),
    Classifier::new(2, 8, 1, 10, [-0.0972417, 0.0152227, 0.129003]),
    Classifier::new(3, 4, 4, 14, [-0.141434, 0.00374515, 0.149935]),
    Classifier::new(5, 4, 2, 15, [-0.64035, -0.466999, -0.285493]),
    Classifier::new(5, 9, 2, 3, [-0.322792, -0.254258, -0.174278]),
    Classifier::new(2, 1, 8, 4, [-0.0741375, -0.00590933, 0.0600357]),
];
const TEST2: [Classifier; 16] = [
    Classifier::new(0, 4, 3, 15, [1.98215, 2.35817, 2.63523]),
    Classifier::new(4, 4, 6, 15, [-1.03809, -0.651211, -0.282167]),
    Classifier::new(1, 0, 4, 16, [-0.298702, 0.119262, 0.558497]),
    Classifier::new(3, 8, 2, 12, [-0.105439, 0.0153946, 0.135898]),
    Classifier::new(3, 4, 4, 8, [-0.142891, 0.0258736, 0.200632]),
    Classifier::new(4, 0, 3, 5, [-0.826319, -0.590612, -0.368214]),
    Classifier::new(1, 2, 2, 9, [-0.557409, -0.233035, 0.0534525]),
    Classifier::new(2, 7, 3, 4, [-0.0646826, 0.00620476, 0.0784847]),
    Classifier::new(2, 6, 2, 16, [-0.192387, -0.029699, 0.215855]),
    Classifier::new(2, 1, 3, 2, [-0.0397818, -0.00568076, 0.0292026]),
    Classifier::new(5, 10, 1, 15, [-0.53823, -0.369934, -0.190235]),
    Classifier::new(3, 6, 2, 10, [-0.124877, 0.0296483, 0.139239]),
    Classifier::new(2, 1, 1, 14, [-0.101475, 0.0225617, 0.231971]),
    Classifier::new(3, 5, 6, 4, [-0.0799915, -0.00729616, 0.063262]),
    Classifier::new(1, 9, 2, 12, [-0.272556, 0.019424, 0.302559]),
    Classifier::new(3, 4, 2, 14, [-0.164292, -0.0321188, 0.0846339]),
];
