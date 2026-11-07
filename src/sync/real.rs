pub(crate) use parking_lot::Mutex;
pub(crate) use std::sync::atomic::{
  AtomicBool, AtomicIsize, AtomicPtr, AtomicU32, AtomicUsize, Ordering, compiler_fence, fence,
};
pub(crate) use std::thread_local;

/// True when a model checker runs the code. Consume-ordered loads become acquire loads then.
pub(crate) const MODELED: bool = false;

/// Entry pointers per node of the concurrent map.
pub(crate) const CAP: usize = 32;

/// Bytes per arena chunk.
pub(crate) const ARENA_CHUNK: usize = 256 * 1024;

/// Tallest tower the concurrent maps draw.
pub(crate) const TOWER_CAP: usize = crate::slab::MAX_HEIGHT;

/// Test-and-test-and-set spinlock for a node. Held for a few dozen nanoseconds.
pub(crate) struct NodeLock(AtomicBool);

impl NodeLock {
  pub(crate) fn new() -> Self {
    NodeLock(AtomicBool::new(false))
  }

  #[inline(always)]
  pub(crate) fn lock(&self) {
    loop {
      if !self.0.swap(true, Ordering::Acquire) {
        return;
      }
      while self.0.load(Ordering::Relaxed) {
        spin_loop();
      }
    }
  }

  #[inline(always)]
  pub(crate) fn unlock(&self) {
    self.0.store(false, Ordering::Release);
  }
}

#[inline(always)]
pub(crate) fn spin_loop() {
  std::hint::spin_loop();
}
