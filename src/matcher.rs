use crate::{Algorithm, Config, Error, hamming_distance};

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub pos1: usize,
    pub pos2: usize,
    pub duration: usize,
    /// Mean bit errors per subfingerprint, in 0..=32.
    pub score: f64,
    pub left_score: f64,
    pub right_score: f64,
}

impl Segment {
    pub fn public_score(&self) -> usize {
        (self.score * 100.0 + 0.5) as usize
    }
    pub fn similarity_percent(&self) -> f64 {
        ((1.0 - self.score / 32.0) * 100.0).clamp(0.0, 100.0)
    }
    /// Merge only segments adjacent in both source fingerprints.
    pub fn merge(&self, other: &Self) -> Option<Self> {
        if self.pos1 + self.duration != other.pos1 || self.pos2 + self.duration != other.pos2 {
            return None;
        }
        let duration = self.duration + other.duration;
        if duration == 0 {
            return None;
        }
        Some(Self {
            pos1: self.pos1,
            pos2: self.pos2,
            duration,
            score: (self.score * self.duration as f64 + other.score * other.duration as f64)
                / duration as f64,
            left_score: self.score,
            right_score: other.score,
        })
    }
}

pub struct Matcher {
    config: Config,
    threshold: f64,
}
impl Matcher {
    pub fn new(algorithm: Algorithm) -> Self {
        Self {
            config: Config { algorithm },
            threshold: 10.0,
        }
    }
    pub fn set_match_threshold(&mut self, threshold: f64) -> Result<(), Error> {
        if !threshold.is_finite() || !(0.0..=32.0).contains(&threshold) {
            return Err(Error::InvalidThreshold);
        }
        self.threshold = threshold;
        Ok(())
    }
    pub fn match_threshold(&self) -> f64 {
        self.threshold
    }
    pub fn hash_time(&self, index: usize) -> f64 {
        self.config.item_duration_seconds() * index as f64
    }
    pub fn hash_duration(&self, index: usize) -> f64 {
        self.hash_time(index) + self.config.delay_seconds()
    }

    pub fn match_fingerprints(&self, first: &[u32], second: &[u32]) -> Result<Vec<Segment>, Error> {
        if first.len() >= 524286 || second.len() >= 524286 {
            return Err(Error::FingerprintTooLong);
        }
        if first.is_empty() || second.is_empty() {
            return Ok(Vec::new());
        }
        let mut offsets: Vec<_> = first
            .iter()
            .enumerate()
            .map(|(i, value)| (value & 0xfff00000) | i as u32)
            .chain(
                second
                    .iter()
                    .enumerate()
                    .map(|(i, value)| (value & 0xfff00000) | 0x80000 | i as u32),
            )
            .collect();
        offsets.sort_unstable();
        let mut histogram = vec![0usize; first.len() + second.len()];
        for (i, offset) in offsets.iter().enumerate() {
            if offset & 0x80000 != 0 {
                continue;
            }
            for other in &offsets[i + 1..] {
                if offset & 0xfff00000 != other & 0xfff00000 {
                    break;
                }
                if other & 0x80000 != 0 {
                    let index =
                        (offset & 0x7ffff) as usize + second.len() - (other & 0x7ffff) as usize;
                    histogram[index] += 1;
                }
            }
        }
        let alignment = histogram
            .iter()
            .enumerate()
            .filter(|(i, count)| {
                **count > 1
                    && (*i == 0 || histogram[i - 1] <= **count)
                    && (*i + 1 == histogram.len() || histogram[i + 1] <= **count)
            })
            .max_by(|(ia, a), (ib, b)| a.cmp(b).then_with(|| ib.cmp(ia)));
        let Some((position, _)) = alignment else {
            return Ok(Vec::new());
        };
        let pos1 = position.saturating_sub(second.len());
        let pos2 = second.len().saturating_sub(position);
        let length = (first.len() - pos1).min(second.len() - pos2);
        let errors: Vec<_> = first[pos1..pos1 + length]
            .iter()
            .zip(&second[pos2..pos2 + length])
            .map(|(a, b)| hamming_distance(*a, *b) as f64)
            .collect();
        let smooth = gaussian(&errors);
        let gradient: Vec<_> = (0..length)
            .map(|i| {
                if length < 2 {
                    0.0
                } else if i == 0 {
                    (smooth[1] - smooth[0]).abs()
                } else if i + 1 == length {
                    (smooth[i] - smooth[i - 1]).abs()
                } else {
                    ((smooth[i + 1] - smooth[i - 1]) / 2.0).abs()
                }
            })
            .collect();
        let mut peaks = Vec::new();
        for i in 1..length.saturating_sub(1) {
            if gradient[i] > 0.15
                && gradient[i] >= gradient[i - 1]
                && gradient[i] >= gradient[i + 1]
                && peaks.last().is_none_or(|previous| previous + 1 < i)
            {
                peaks.push(i);
            }
        }
        peaks.push(length);
        let mut begin = 0;
        let mut segments: Vec<Segment> = Vec::new();
        for end in peaks {
            let duration = end - begin;
            let score = errors[begin..end].iter().sum::<f64>() / duration as f64;
            if score < self.threshold {
                let next = Segment {
                    pos1: pos1 + begin,
                    pos2: pos2 + begin,
                    duration,
                    score,
                    left_score: score,
                    right_score: score,
                };
                let merged = segments
                    .last()
                    .filter(|last| (last.score - score).abs() < 0.7)
                    .and_then(|last| last.merge(&next));
                if let Some(merged) = merged {
                    *segments.last_mut().unwrap() = merged;
                } else {
                    segments.push(next);
                }
            }
            begin = end;
        }
        Ok(segments)
    }
}

fn reflected(index: isize, length: usize) -> usize {
    let position = index.rem_euclid(2 * length as isize) as usize;
    if position >= length {
        2 * length - position - 1
    } else {
        position
    }
}

fn box_filter(input: &[f64], width: usize) -> Vec<f64> {
    let half = (width / 2) as isize;
    let mut sum: f64 = (-half..width as isize - half)
        .map(|i| input[reflected(i, input.len())])
        .sum();
    (0..input.len())
        .map(|i| {
            let value = sum / width as f64;
            sum += input[reflected(i as isize + width as isize - half, input.len())]
                - input[reflected(i as isize - half, input.len())];
            value
        })
        .collect()
}

fn gaussian(input: &[f64]) -> Vec<f64> {
    // sigma=8, three passes: the reference width selection gives 15,15,17.
    [15, 15, 17]
        .into_iter()
        .fold(input.to_vec(), |data, width| box_filter(&data, width))
}
