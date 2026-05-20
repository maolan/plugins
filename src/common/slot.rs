use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU64, Ordering};

/// Lock-free single-writer multi-reader data slot using a sequence lock.
///
/// - **One writer only**. Multiple concurrent writers will panic in debug builds.
/// - **Any number of readers**. Readers never block and never contend with each other.
/// - `T` must be `Copy` so that torn reads are merely garbage values, not UB.
///
/// Typical usage:
/// ```ignore
/// let slot = SeqLockSlot::new(FftData::default());
/// // Writer (audio thread)
/// slot.write(|fft| compute_fft(&audio, &mut fft.bins));
/// // Reader (UI thread)
/// let mut fft = FftData::default();
/// if slot.read(&mut fft) { draw(&fft); }
/// ```
pub struct SeqLockSlot<T> {
    seq: AtomicU64,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Send for SeqLockSlot<T> {}
unsafe impl<T: Send> Sync for SeqLockSlot<T> {}

impl<T> SeqLockSlot<T> {
    pub fn new(data: T) -> Self {
        Self {
            seq: AtomicU64::new(0),
            data: UnsafeCell::new(data),
        }
    }

    /// Writer: mutate the slot under the sequence lock.
    ///
    /// Must be called from exactly one thread. Panics in debug mode if two
    /// writers race.
    pub fn write(&self, f: impl FnOnce(&mut T)) {
        let old = self.seq.fetch_add(1, Ordering::Relaxed);
        debug_assert_eq!(old % 2, 0, "SeqLockSlot: concurrent writers detected");

        // SAFETY: seq is now odd, readers will retry until it flips back to even.
        unsafe { f(&mut *self.data.get()) };

        self.seq.fetch_add(1, Ordering::Release);
    }

    /// Reader: attempt to copy the current value into `out`.
    ///
    /// Returns `true` if the snapshot was consistent (no writer intervened).
    /// Returns `false` if a writer was active or completed during the copy;
    /// `out` may contain garbage and should be discarded.
    pub fn read(&self, out: &mut T) -> bool
    where
        T: Copy,
    {
        let seq_before = self.seq.load(Ordering::Acquire);
        if seq_before % 2 != 0 {
            return false;
        }

        // SAFETY: seq is even, so writer is not active. We copy the value
        // byte-for-byte. If the writer starts immediately after this load,
        // we will detect it via seq_after and discard the result.
        unsafe { std::ptr::copy_nonoverlapping(self.data.get(), out, 1) };

        let seq_after = self.seq.load(Ordering::Acquire);
        seq_before == seq_after
    }

    /// Reader: spin until a consistent snapshot is obtained.
    ///
    /// Use sparingly — prefer `read` with a retry loop or drop the frame.
    pub fn read_spin(&self, out: &mut T)
    where
        T: Copy,
    {
        while !self.read(out) {
            std::hint::spin_loop();
        }
    }
}
