//! Generation-tagged background job slot: newest request wins, stale results are dropped.

use std::sync::mpsc::{self, Receiver, Sender};
#[cfg(test)]
use std::time::{Duration, Instant};

/// One logical background job (summary, histogram, …). Each [`Self::start`]
/// or [`Self::cancel`] bumps the generation; results sent through an older
/// [`JobTicket`] are discarded on arrival.
pub struct BgJob<T> {
    gen: u64,
    tx: Sender<(u64, T)>,
    rx: Receiver<(u64, T)>,
}

/// Send handle for one job request; moved into the worker thread.
pub struct JobTicket<T> {
    gen: u64,
    tx: Sender<(u64, T)>,
}

impl<T> JobTicket<T> {
    pub fn send(self, value: T) {
        let _ = self.tx.send((self.gen, value));
    }
}

impl<T> BgJob<T> {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self { gen: 0, tx, rx }
    }

    /// Invalidate in-flight work and hand out a ticket for a new request.
    pub fn start(&mut self) -> JobTicket<T> {
        self.cancel();
        self.ticket()
    }

    /// Invalidate in-flight work without starting a new request.
    pub fn cancel(&mut self) {
        self.gen = self.gen.wrapping_add(1);
    }

    /// Ticket for the current generation (tests simulate late workers with it).
    pub fn ticket(&self) -> JobTicket<T> {
        JobTicket {
            gen: self.gen,
            tx: self.tx.clone(),
        }
    }

    /// Next result for the current generation, skipping stale ones. Non-blocking.
    pub fn poll(&mut self) -> Option<T> {
        while let Ok((gen, value)) = self.rx.try_recv() {
            if gen == self.gen {
                return Some(value);
            }
        }
        None
    }

    /// Block up to `timeout` for a result of the current generation (tests).
    #[cfg(test)]
    pub fn wait(&mut self, timeout: Duration) -> Option<T> {
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            match self.rx.recv_timeout(remaining) {
                Ok((gen, value)) if gen == self.gen => return Some(value),
                Ok(_) => continue,
                Err(_) => return None,
            }
        }
    }
}

impl<T> Default for BgJob<T> {
    fn default() -> Self {
        Self::new()
    }
}
