use std::collections::VecDeque;

/// Output sampling strategy for managing large result sets.
///
/// Three modes:
/// - PassThrough: emit all entries (default)
/// - HeadTail: emit first H, buffer last T, skip middle
/// - Reservoir: uniform random sample of N entries
pub struct Sampler {
    mode: SampleMode,
    emitted: usize,
    total: usize,
    tail_buf: VecDeque<String>,
    reservoir: Vec<(usize, String)>,
    rng: u64,
}

enum SampleMode {
    PassThrough,
    HeadTail { head: usize, tail: usize },
    Reservoir(usize),
}

pub struct SamplerResult {
    pub lines: Vec<String>,
    /// Human-readable context message (e.g. "showing last 50 of 1000 matched entries")
    pub header: Option<String>,
}

impl Sampler {
    /// Create a sampler from CLI flags.
    ///
    /// - `tail > 0`: HeadTail mode (head = limit, tail = tail)
    /// - `sample > 0`: Reservoir mode
    /// - otherwise: PassThrough (limit handled by early-exit in caller)
    pub fn new(tail: usize, sample: usize, limit: usize) -> Self {
        let mode = if sample > 0 {
            SampleMode::Reservoir(sample)
        } else if tail > 0 {
            SampleMode::HeadTail { head: limit, tail }
        } else {
            SampleMode::PassThrough
        };
        Self {
            mode,
            emitted: 0,
            total: 0,
            tail_buf: VecDeque::with_capacity(if tail > 0 { tail.min(10000) + 1 } else { 0 }),
            reservoir: if sample > 0 { Vec::with_capacity(sample) } else { Vec::new() },
            rng: 42,
        }
    }

    /// Feed a matched entry. Returns true if it should be output immediately.
    pub fn should_emit(&mut self, line: &str) -> bool {
        self.total += 1;
        match self.mode {
            SampleMode::PassThrough => true,
            SampleMode::HeadTail { head, tail } => {
                if head > 0 && self.emitted < head {
                    self.emitted += 1;
                    true
                } else {
                    if self.tail_buf.len() == tail {
                        self.tail_buf.pop_front();
                    }
                    self.tail_buf.push_back(line.to_string());
                    false
                }
            }
            SampleMode::Reservoir(size) => {
                let idx = self.total - 1;
                if self.reservoir.len() < size {
                    self.reservoir.push((idx, line.to_string()));
                } else {
                    let r = self.next_rand() as usize % self.total;
                    if r < size {
                        self.reservoir[r] = (idx, line.to_string());
                    }
                }
                false
            }
        }
    }

    /// Whether the caller must process all input (no early exit).
    pub fn needs_full_scan(&self) -> bool {
        !matches!(self.mode, SampleMode::PassThrough)
    }

    /// Consume and return buffered lines + context header.
    pub fn finish(mut self) -> SamplerResult {
        match self.mode {
            SampleMode::PassThrough => SamplerResult { lines: Vec::new(), header: None },
            SampleMode::HeadTail { head, .. } => {
                let tail_count = self.tail_buf.len();
                let skipped = self.total.saturating_sub(self.emitted + tail_count);
                let header = if skipped > 0 {
                    if head > 0 {
                        Some(format!("{skipped} entries omitted ({} total matched)", self.total))
                    } else {
                        Some(format!("showing last {tail_count} of {} matched entries", self.total))
                    }
                } else {
                    None
                };
                SamplerResult {
                    lines: self.tail_buf.into_iter().collect(),
                    header,
                }
            }
            SampleMode::Reservoir(_) => {
                self.reservoir.sort_by_key(|(idx, _)| *idx);
                let count = self.reservoir.len();
                let header = if self.total > count {
                    Some(format!("sampled {count} of {} matched entries", self.total))
                } else {
                    None
                };
                SamplerResult {
                    lines: self.reservoir.into_iter().map(|(_, line)| line).collect(),
                    header,
                }
            }
        }
    }

    fn next_rand(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }
}
