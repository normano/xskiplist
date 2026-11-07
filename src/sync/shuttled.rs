pub(crate) use shuttle::sync::atomic::{AtomicBool, AtomicIsize, AtomicPtr, AtomicU32, AtomicUsize, Ordering, fence};
pub(crate) use shuttle::thread_local;
pub(crate) use std::sync::atomic::compiler_fence;

pub(crate) const MODELED: bool = true;

/// Four entries per node, so a few dozen keys split and empty nodes often.
pub(crate) const CAP: usize = 4;

pub(crate) const ARENA_CHUNK: usize = 4096;

pub(crate) const TOWER_CAP: usize = 6;

/// A node lock over shuttle's mutex, so a waiting thread blocks in the schedule instead of
/// spinning through it. The guard is parked in the lock while it is held.
pub(crate) struct NodeLock {
  mutex: shuttle::sync::Mutex<()>,
  guard: std::cell::UnsafeCell<Option<shuttle::sync::MutexGuard<'static, ()>>>,
}

unsafe impl Send for NodeLock {}
unsafe impl Sync for NodeLock {}

impl NodeLock {
  pub(crate) fn new() -> Self {
    NodeLock {
      mutex: shuttle::sync::Mutex::new(()),
      guard: std::cell::UnsafeCell::new(None),
    }
  }

  pub(crate) fn lock(&self) {
    let guard = self.mutex.lock().unwrap();
    let guard: shuttle::sync::MutexGuard<'static, ()> = unsafe { std::mem::transmute(guard) };
    unsafe { *self.guard.get() = Some(guard) }
  }

  pub(crate) fn unlock(&self) {
    unsafe { (*self.guard.get()).take() };
  }
}

#[inline(always)]
pub(crate) fn spin_loop() {
  shuttle::thread::yield_now();
}

/// `parking_lot::Mutex` shape over shuttle's mutex: `lock` returns the guard.
pub(crate) struct Mutex<T>(shuttle::sync::Mutex<T>);

impl<T> Mutex<T> {
  pub(crate) fn new(value: T) -> Self {
    Mutex(shuttle::sync::Mutex::new(value))
  }

  pub(crate) fn lock(&self) -> shuttle::sync::MutexGuard<'_, T> {
    self.0.lock().unwrap()
  }
}
