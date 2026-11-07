pub(crate) use loom::sync::atomic::{AtomicBool, AtomicIsize, AtomicPtr, AtomicU32, AtomicUsize, Ordering, fence};
pub(crate) use loom::thread_local;
pub(crate) use std::sync::atomic::compiler_fence;

pub(crate) const MODELED: bool = true;

/// Two entries per node, so three keys split a node and two removals empty one.
pub(crate) const CAP: usize = 2;

/// A chunk holds a handful of nodes, so a model with a few inserts refills a stripe.
pub(crate) const ARENA_CHUNK: usize = 1024;

/// Three levels keep the number of atomics a model explores small.
pub(crate) const TOWER_CAP: usize = 3;

/// A node lock over loom's mutex, so a thread waiting for a node blocks in the model instead of
/// spinning through it. The guard is parked in the lock while it is held.
pub(crate) struct NodeLock {
  mutex: loom::sync::Mutex<()>,
  guard: std::cell::UnsafeCell<Option<loom::sync::MutexGuard<'static, ()>>>,
}

unsafe impl Send for NodeLock {}
unsafe impl Sync for NodeLock {}

impl NodeLock {
  pub(crate) fn new() -> Self {
    NodeLock {
      mutex: loom::sync::Mutex::new(()),
      guard: std::cell::UnsafeCell::new(None),
    }
  }

  pub(crate) fn lock(&self) {
    let guard = self.mutex.lock().unwrap();
    let guard: loom::sync::MutexGuard<'static, ()> = unsafe { std::mem::transmute(guard) };
    unsafe { *self.guard.get() = Some(guard) }
  }

  pub(crate) fn unlock(&self) {
    unsafe { (*self.guard.get()).take() };
  }
}

#[inline(always)]
pub(crate) fn spin_loop() {
  loom::thread::yield_now();
}

/// `parking_lot::Mutex` shape over loom's mutex: `lock` returns the guard.
pub(crate) struct Mutex<T>(loom::sync::Mutex<T>);

impl<T> Mutex<T> {
  pub(crate) fn new(value: T) -> Self {
    Mutex(loom::sync::Mutex::new(value))
  }

  pub(crate) fn lock(&self) -> loom::sync::MutexGuard<'_, T> {
    self.0.lock().unwrap()
  }
}
