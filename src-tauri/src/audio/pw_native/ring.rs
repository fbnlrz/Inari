//! Minimal lock-free SPSC ring buffer for f32 samples, connecting the mic
//! capture callback (producer) to the virtual-source playback callback
//! (consumer). Both run on PipeWire data threads; no locks, no allocation.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

pub struct Ring {
    buf: Box<[AtomicU32]>,
    /// Next write position (producer-owned).
    write: AtomicUsize,
    /// Next read position (consumer-owned).
    read: AtomicUsize,
}

impl Ring {
    /// Capacity is rounded up to a power of two; the effective window is
    /// `capacity - 1` samples - the spare slot is the margin `pop`
    /// resynchronises to after an overrun.
    pub fn new(capacity: usize) -> Self {
        let cap = capacity.next_power_of_two().max(2);
        let buf = (0..cap).map(|_| AtomicU32::new(0)).collect::<Vec<_>>();
        Self {
            buf: buf.into_boxed_slice(),
            write: AtomicUsize::new(0),
            read: AtomicUsize::new(0),
        }
    }

    fn mask(&self) -> usize {
        self.buf.len() - 1
    }

    /// Push samples. Overflow is not handled here on purpose: the producer
    /// touches `write` and nothing else, so the SPSC discipline holds even
    /// when it laps the consumer. The consumer detects the lap in `pop`.
    pub fn push(&self, samples: &[f32]) {
        let mask = self.mask();
        let mut w = self.write.load(Ordering::Relaxed);
        for &s in samples {
            self.buf[w & mask].store(s.to_bits(), Ordering::Relaxed);
            w = w.wrapping_add(1);
        }
        self.write.store(w, Ordering::Release);
    }

    /// Pop up to `out.len()` samples; unfilled tail is zeroed (underrun).
    /// Returns the number of real samples written.
    pub fn pop(&self, out: &mut [f32]) -> usize {
        let mask = self.mask();
        let cap = self.buf.len();
        let w = self.write.load(Ordering::Acquire);
        let mut r = self.read.load(Ordering::Relaxed);
        // Overrun: the producer lapped us, so everything older than the last
        // `cap - 1` samples is already overwritten. Skip the stale span and
        // resynchronise onto the freshest window - for live audio stale
        // samples are worthless, and reading them would return torn data.
        if w.wrapping_sub(r) >= cap {
            r = w.wrapping_sub(cap - 1);
        }
        let avail = w.wrapping_sub(r).min(out.len());
        for slot in out.iter_mut().take(avail) {
            *slot = f32::from_bits(self.buf[r & mask].load(Ordering::Relaxed));
            r = r.wrapping_add(1);
        }
        self.read.store(r, Ordering::Release);
        for slot in out.iter_mut().skip(avail) {
            *slot = 0.0;
        }
        avail
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_underrun() {
        let ring = Ring::new(8);
        ring.push(&[1.0, 2.0, 3.0]);
        let mut out = [0.0f32; 5];
        let n = ring.pop(&mut out);
        assert_eq!(n, 3);
        assert_eq!(&out[..3], &[1.0, 2.0, 3.0]);
        assert_eq!(&out[3..], &[0.0, 0.0]); // underrun zero-fill
    }

    #[test]
    fn overflow_keeps_freshest_window(// producer overruns consumer
    ) {
        let ring = Ring::new(4); // effective window of 3
        let data: Vec<f32> = (0..10).map(|i| i as f32).collect();
        ring.push(&data);
        let mut out = [0.0f32; 4];
        let n = ring.pop(&mut out);
        assert_eq!(n, 3);
        assert_eq!(&out[..3], &[7.0, 8.0, 9.0]); // freshest 3 survive
        assert_eq!(out[3], 0.0); // tail zero-filled
    }

    #[test]
    fn producer_never_writes_the_read_cursor() {
        // After an overrun the consumer resynchronises and then keeps
        // reading forward - the stale window must not come back.
        let ring = Ring::new(4);
        ring.push(&(0..10).map(|i| i as f32).collect::<Vec<_>>());
        let mut out = [0.0f32; 3];
        assert_eq!(ring.pop(&mut out), 3);
        assert_eq!(out, [7.0, 8.0, 9.0]);
        ring.push(&[10.0, 11.0]);
        assert_eq!(ring.pop(&mut out), 2);
        assert_eq!(&out[..2], &[10.0, 11.0]);
    }
}
