use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU64, Ordering};

#[repr(C)]
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

    pub fn write(&self, f: impl FnOnce(&mut T)) {
        let old = self.seq.fetch_add(1, Ordering::Relaxed);
        debug_assert_eq!(old % 2, 0, "SeqLockSlot: concurrent writers detected");

        unsafe { f(&mut *self.data.get()) };

        self.seq.fetch_add(1, Ordering::Release);
    }

    pub fn read(&self, out: &mut T) -> bool
    where
        T: Copy,
    {
        let seq_before = self.seq.load(Ordering::Acquire);
        if !seq_before.is_multiple_of(2) {
            return false;
        }

        unsafe { std::ptr::copy_nonoverlapping(self.data.get(), out, 1) };

        let seq_after = self.seq.load(Ordering::Acquire);
        seq_before == seq_after
    }

    pub fn read_spin(&self, out: &mut T)
    where
        T: Copy,
    {
        while !self.read(out) {
            std::hint::spin_loop();
        }
    }
}
