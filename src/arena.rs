// loom's `thread_local!` cannot take `const` initialisers.
#![allow(clippy::missing_const_for_thread_local)]

use crate::node::Levels;
use crate::slab::MAX_HEIGHT;
use crate::sync::{
  ARENA_CHUNK, AtomicPtr, AtomicUsize, MODELED, Mutex, Ordering, TOWER_CAP, compiler_fence, thread_local,
};
use std::alloc::{self, Layout};
use std::borrow::Borrow;
use std::cell::Cell;
use std::fmt;
use std::marker::PhantomData;
use std::mem::{self, align_of, size_of};
use std::ops::{Bound, RangeBounds};
use std::ptr::{self, NonNull};

#[cfg(test)]
mod tests;

const STRIPES: usize = 16;

// loom's `thread_local!` takes a plain expression, not a `const` block.
thread_local! {
  static STRIPE: Cell<usize> = Cell::new(usize::MAX);
  static LEVELS: Cell<Option<Levels>> = Cell::new(None);
}

fn stripe() -> usize {
  if MODELED {
    return 0;
  }
  STRIPE.with(|s| {
    let v = s.get();
    if v != usize::MAX {
      return v;
    }
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let v = NEXT.fetch_add(1, Ordering::Relaxed) % STRIPES;
    s.set(v);
    v
  })
}

fn next_height() -> usize {
  LEVELS.with(|l| {
    let mut levels = l
      .take()
      .unwrap_or_else(|| if MODELED { Levels::seeded(0x5EED) } else { Levels::new() });
    let h = levels.next_half().min(TOWER_CAP);
    l.set(Some(levels));
    h
  })
}

/// A stripe's cursor is `generation << 32 | offset` into its current chunk, or `PARKED` while a
/// refill is replacing the chunk. The generation makes every bump taken before a refill fail.
const PARKED: usize = usize::MAX;
const OFFSET_MASK: usize = 0xffff_ffff;

#[repr(align(128))]
struct Stripe {
  cursor: AtomicUsize,
  base: AtomicPtr<u8>,
  end: AtomicUsize,
  refill: Mutex<usize>,
}

/// Bump allocator over chunks that are freed only when the arena drops.
struct Arena {
  stripes: Box<[Stripe; STRIPES]>,
  chunks: Mutex<Vec<(NonNull<u8>, Layout)>>,
}

unsafe impl Send for Arena {}
unsafe impl Sync for Arena {}

impl Arena {
  fn new() -> Self {
    Arena {
      stripes: Box::new(std::array::from_fn(|_| Stripe {
        cursor: AtomicUsize::new(PARKED),
        base: AtomicPtr::new(ptr::null_mut()),
        end: AtomicUsize::new(0),
        refill: Mutex::new(0),
      })),
      chunks: Mutex::new(Vec::new()),
    }
  }

  fn alloc(&self, layout: Layout) -> NonNull<u8> {
    debug_assert!(layout.align() <= 128);
    let stripe = &self.stripes[stripe()];
    loop {
      let cursor = stripe.cursor.load(Ordering::Acquire);
      if cursor != PARKED {
        let start = ((cursor & OFFSET_MASK) + layout.align() - 1) & !(layout.align() - 1);
        let next = start + layout.size();
        if next <= stripe.end.load(Ordering::Relaxed) {
          let base = stripe.base.load(Ordering::Relaxed);
          let bumped = (cursor & !OFFSET_MASK) | next;
          if stripe
            .cursor
            .compare_exchange_weak(cursor, bumped, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
          {
            return unsafe { NonNull::new_unchecked(base.add(start)) };
          }
          continue;
        }
      }
      let mut generation = stripe.refill.lock();
      if stripe.cursor.load(Ordering::Relaxed) != cursor {
        continue;
      }
      stripe.cursor.store(PARKED, Ordering::Relaxed);
      let size = ARENA_CHUNK.max(layout.size() + layout.align());
      let chunk_layout = Layout::from_size_align(size, 128).expect("arena chunk layout");
      let chunk = unsafe { alloc::alloc(chunk_layout) };
      let chunk = NonNull::new(chunk).unwrap_or_else(|| alloc::handle_alloc_error(chunk_layout));
      self.chunks.lock().push((chunk, chunk_layout));
      stripe.base.store(chunk.as_ptr(), Ordering::Relaxed);
      stripe.end.store(size, Ordering::Relaxed);
      *generation += 1;
      stripe.cursor.store(*generation << 32, Ordering::Release);
    }
  }
}

impl Drop for Arena {
  fn drop(&mut self) {
    for (chunk, layout) in self.chunks.lock().drain(..) {
      unsafe { alloc::dealloc(chunk.as_ptr(), layout) }
    }
  }
}

/// Entry node: `height` atomic forward pointers laid out in reverse level order directly before
/// this header.
#[repr(C)]
struct Node<K, V> {
  key: K,
  value: V,
  height: u32,
}

type Ptr<K, V> = *mut Node<K, V>;

#[inline(always)]
unsafe fn tower_slot<K, V>(node: Ptr<K, V>, level: usize) -> *mut AtomicPtr<Node<K, V>> {
  unsafe { node.cast::<AtomicPtr<Node<K, V>>>().sub(level + 1) }
}

#[inline(always)]
unsafe fn tower<'a, K, V>(node: Ptr<K, V>, level: usize) -> &'a AtomicPtr<Node<K, V>> {
  unsafe { &*tower_slot(node, level) }
}

/// Consume-ordered load of a forward pointer: the fields of the node it points to were written
/// before the pointer was published with release ordering and are read only through it, so the
/// address dependency orders them on every supported target. Miri, ThreadSanitizer and the model
/// checkers have no dependency ordering and get an acquire load.
#[inline(always)]
unsafe fn next<K, V>(node: Ptr<K, V>, level: usize) -> Ptr<K, V> {
  unsafe {
    if cfg!(miri) || cfg!(xskiplist_tsan) || MODELED {
      tower(node, level).load(Ordering::Acquire)
    } else {
      let p = tower(node, level).load(Ordering::Relaxed);
      compiler_fence(Ordering::Acquire);
      p
    }
  }
}

/// Walks right from `x` at `level` while the next node's key passes `go_right`. Returns the last
/// node that passed (or `x`) and the first that did not (null at the end). The forward pointer of
/// a node is loaded together with its key so that one step costs one dependent round trip.
#[inline(always)]
unsafe fn walk<K, V>(x: Ptr<K, V>, level: usize, mut go_right: impl FnMut(&K) -> bool) -> (Ptr<K, V>, Ptr<K, V>) {
  unsafe {
    let mut x = x;
    let mut n = next(x, level);
    while !n.is_null() {
      let succ = next(n, level);
      if go_right(&(*n).key) {
        x = n;
        n = succ;
      } else {
        break;
      }
    }
    (x, n)
  }
}

fn node_layout<K, V>(height: usize) -> (Layout, usize) {
  let align = align_of::<Node<K, V>>().max(align_of::<AtomicPtr<Node<K, V>>>());
  let offset = (size_of::<AtomicPtr<Node<K, V>>>() * height + align - 1) & !(align - 1);
  let layout = Layout::from_size_align(offset + size_of::<Node<K, V>>(), align).expect("node layout");
  (layout, offset)
}

/// Striped counter: one cache line per stripe so that writers on different threads do not share
/// a line.
struct Counter(Box<[Padded; STRIPES]>);

#[repr(align(128))]
struct Padded(AtomicUsize);

impl Counter {
  fn new() -> Self {
    Counter(Box::new(std::array::from_fn(|_| Padded(AtomicUsize::new(0)))))
  }

  #[inline(always)]
  fn add(&self, n: usize) {
    self.0[stripe()].0.fetch_add(n, Ordering::Relaxed);
  }

  fn sum(&self) -> usize {
    self.0.iter().map(|p| p.0.load(Ordering::Relaxed)).sum()
  }
}

/// A concurrent sorted map that only grows. Inserts and reads run in parallel without locks or
/// guards: nothing is removed before the map drops, so every reference it hands out lives as long
/// as the borrow of the map.
pub struct ArenaSkipMap<K, V> {
  head: Ptr<K, V>,
  height: AtomicUsize,
  len: Counter,
  arena: Arena,
  marker: PhantomData<(K, V)>,
}

unsafe impl<K: Send + Sync, V: Send + Sync> Send for ArenaSkipMap<K, V> {}
unsafe impl<K: Send + Sync, V: Send + Sync> Sync for ArenaSkipMap<K, V> {}

impl<K, V> ArenaSkipMap<K, V> {
  /// Makes a new, empty map.
  pub fn new() -> Self {
    let arena = Arena::new();
    let (layout, offset) = node_layout::<K, V>(TOWER_CAP);
    let head = unsafe {
      let base = arena.alloc(layout).as_ptr();
      let head: Ptr<K, V> = base.add(offset).cast();
      for level in 0..TOWER_CAP {
        tower_slot(head, level).write(AtomicPtr::new(ptr::null_mut()));
      }
      ptr::addr_of_mut!((*head).height).write(TOWER_CAP as u32);
      head
    };
    ArenaSkipMap {
      head,
      height: AtomicUsize::new(1),
      len: Counter::new(),
      arena,
      marker: PhantomData,
    }
  }

  /// Returns the number of entries inserted so far, as seen by the calling thread.
  pub fn len(&self) -> usize {
    self.len.sum()
  }

  /// Returns `true` if no entry has been inserted.
  pub fn is_empty(&self) -> bool {
    unsafe { next(self.head, 0).is_null() }
  }

  /// The node whose key compares equal, if any.
  #[inline(always)]
  unsafe fn find<Q>(&self, key: &Q) -> Ptr<K, V>
  where
    K: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    unsafe {
      let n = self.lower_bound(key);
      if !n.is_null() && (*n).key.borrow() == key {
        n
      } else {
        ptr::null_mut()
      }
    }
  }

  /// The first node whose key is not less than `key` (null if none).
  #[inline(always)]
  unsafe fn lower_bound<Q>(&self, key: &Q) -> Ptr<K, V>
  where
    K: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    unsafe {
      let mut x = self.head;
      let mut n = ptr::null_mut();
      let mut level = self.height.load(Ordering::Relaxed);
      while level > 0 {
        level -= 1;
        (x, n) = walk(x, level, |k| k.borrow() < key);
      }
      n
    }
  }

  /// The first node whose key is greater than `key` (null if none).
  #[inline(always)]
  unsafe fn upper_bound<Q>(&self, key: &Q) -> Ptr<K, V>
  where
    K: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    unsafe {
      let mut x = self.head;
      let mut n = ptr::null_mut();
      let mut level = self.height.load(Ordering::Relaxed);
      while level > 0 {
        level -= 1;
        (x, n) = walk(x, level, |k| k.borrow() <= key);
      }
      n
    }
  }

  /// Returns the value for `key`.
  pub fn get<Q>(&self, key: &Q) -> Option<&V>
  where
    K: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    unsafe {
      let n = self.find(key);
      if n.is_null() { None } else { Some(&(*n).value) }
    }
  }

  /// Returns the stored key and the value for `key`.
  pub fn get_key_value<Q>(&self, key: &Q) -> Option<(&K, &V)>
  where
    K: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    unsafe {
      let n = self.find(key);
      if n.is_null() {
        None
      } else {
        Some((&(*n).key, &(*n).value))
      }
    }
  }

  /// Returns `true` if `key` is present.
  pub fn contains_key<Q>(&self, key: &Q) -> bool
  where
    K: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    unsafe { !self.find(key).is_null() }
  }

  /// Inserts `key` with `value` unless the key is present. Returns the stored entry and whether
  /// this call inserted it.
  pub fn insert(&self, key: K, value: V) -> (&K, &V, bool)
  where
    K: Ord,
  {
    unsafe {
      let mut preds: [Ptr<K, V>; MAX_HEIGHT] = [ptr::null_mut(); MAX_HEIGHT];
      let mut succs: [Ptr<K, V>; MAX_HEIGHT] = [ptr::null_mut(); MAX_HEIGHT];
      let mut x = self.head;
      let mut level = TOWER_CAP;
      while level > 0 {
        level -= 1;
        let (p, s) = walk(x, level, |k| k < &key);
        x = p;
        preds[level] = p;
        succs[level] = s;
      }
      let n = succs[0];
      if !n.is_null() && (*n).key == key {
        return (&(*n).key, &(*n).value, false);
      }
      let h = next_height();
      let (layout, offset) = node_layout::<K, V>(h);
      let node: Ptr<K, V> = self.arena.alloc(layout).as_ptr().add(offset).cast();
      ptr::addr_of_mut!((*node).key).write(key);
      ptr::addr_of_mut!((*node).value).write(value);
      ptr::addr_of_mut!((*node).height).write(h as u32);
      for (level, &succ) in succs.iter().enumerate().take(h) {
        tower_slot(node, level).write(AtomicPtr::new(succ));
      }
      let key = &(*node).key;
      for level in 0..h {
        loop {
          let pred = preds[level];
          let succ = succs[level];
          if tower(pred, level)
            .compare_exchange(succ, node, Ordering::Release, Ordering::Relaxed)
            .is_ok()
          {
            break;
          }
          let (p, s) = walk(pred, level, |k| k < key);
          if level == 0 && !s.is_null() && (*s).key == *key {
            ptr::drop_in_place(ptr::addr_of_mut!((*node).key));
            ptr::drop_in_place(ptr::addr_of_mut!((*node).value));
            return (&(*s).key, &(*s).value, false);
          }
          preds[level] = p;
          succs[level] = s;
          tower(node, level).store(s, Ordering::Relaxed);
        }
      }
      self.height.fetch_max(h, Ordering::Relaxed);
      self.len.add(1);
      (&(*node).key, &(*node).value, true)
    }
  }

  /// Returns the value for `key`, inserting `value` first if the key is absent.
  pub fn get_or_insert(&self, key: K, value: V) -> &V
  where
    K: Ord,
  {
    self.insert(key, value).1
  }

  /// Returns the value for `key`, inserting the result of `f` first if the key is absent. `f` runs
  /// only when the key is absent at the time of the call.
  pub fn get_or_insert_with<F>(&self, key: K, f: F) -> &V
  where
    K: Ord,
    F: FnOnce() -> V,
  {
    if let Some(v) = self.get(&key) {
      return v;
    }
    self.insert(key, f()).1
  }

  /// The smallest entry.
  pub fn first(&self) -> Option<(&K, &V)> {
    unsafe {
      let n = next(self.head, 0);
      if n.is_null() {
        None
      } else {
        Some((&(*n).key, &(*n).value))
      }
    }
  }

  /// The largest entry at the time of the call.
  pub fn last(&self) -> Option<(&K, &V)> {
    unsafe {
      let mut x = self.head;
      let mut level = self.height.load(Ordering::Relaxed);
      while level > 0 {
        level -= 1;
        (x, _) = walk(x, level, |_| true);
      }
      if x == self.head {
        None
      } else {
        Some((&(*x).key, &(*x).value))
      }
    }
  }

  /// Iterates over the entries in key order. Entries inserted during the iteration may or may not
  /// be visited.
  pub fn iter(&self) -> Iter<'_, K, V> {
    Iter {
      next: unsafe { next(self.head, 0) },
      end: ptr::null_mut(),
      marker: PhantomData,
    }
  }

  /// Iterates over the entries whose keys lie in `range`.
  pub fn range<Q, R>(&self, range: R) -> Iter<'_, K, V>
  where
    K: Borrow<Q>,
    Q: Ord + ?Sized,
    R: RangeBounds<Q>,
  {
    unsafe {
      let start = match range.start_bound() {
        Bound::Included(k) => self.lower_bound(k),
        Bound::Excluded(k) => self.upper_bound(k),
        Bound::Unbounded => next(self.head, 0),
      };
      let end = match range.end_bound() {
        Bound::Included(k) => self.upper_bound(k),
        Bound::Excluded(k) => self.lower_bound(k),
        Bound::Unbounded => ptr::null_mut(),
      };
      let next = if !end.is_null() && (!start.is_null() && (*start).key.borrow() >= (*end).key.borrow()) {
        end
      } else {
        start
      };
      Iter {
        next,
        end,
        marker: PhantomData,
      }
    }
  }

  #[cfg(any(test, feature = "verify"))]
  #[doc(hidden)]
  pub fn check_invariants(&self)
  where
    K: Ord,
  {
    unsafe {
      let mut count = 0;
      let mut x = next(self.head, 0);
      let mut prev: Option<&K> = None;
      let mut heights = Vec::new();
      while !x.is_null() {
        if let Some(p) = prev {
          assert!(p < &(*x).key, "keys out of order");
        }
        prev = Some(&(*x).key);
        heights.push((x, (*x).height as usize));
        count += 1;
        x = next(x, 0);
      }
      assert_eq!(count, self.len(), "len");
      for level in 1..TOWER_CAP {
        let expected: Vec<Ptr<K, V>> = heights.iter().filter(|(_, h)| *h > level).map(|(n, _)| *n).collect();
        let mut actual = Vec::new();
        let mut x = next(self.head, level);
        while !x.is_null() {
          actual.push(x);
          x = next(x, level);
        }
        assert_eq!(actual, expected, "level {level}");
      }
    }
  }
}

impl<K, V> Default for ArenaSkipMap<K, V> {
  fn default() -> Self {
    ArenaSkipMap::new()
  }
}

impl<K, V> Drop for ArenaSkipMap<K, V> {
  fn drop(&mut self) {
    if mem::needs_drop::<K>() || mem::needs_drop::<V>() {
      unsafe {
        let mut x = next(self.head, 0);
        while !x.is_null() {
          let after = next(x, 0);
          ptr::drop_in_place(ptr::addr_of_mut!((*x).key));
          ptr::drop_in_place(ptr::addr_of_mut!((*x).value));
          x = after;
        }
      }
    }
  }
}

impl<K: Ord, V> FromIterator<(K, V)> for ArenaSkipMap<K, V> {
  fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
    let map = ArenaSkipMap::new();
    for (k, v) in iter {
      map.insert(k, v);
    }
    map
  }
}

impl<K: fmt::Debug, V: fmt::Debug> fmt::Debug for ArenaSkipMap<K, V> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_map().entries(self.iter()).finish()
  }
}

impl<'a, K, V> IntoIterator for &'a ArenaSkipMap<K, V> {
  type Item = (&'a K, &'a V);
  type IntoIter = Iter<'a, K, V>;

  fn into_iter(self) -> Iter<'a, K, V> {
    self.iter()
  }
}

/// Iterator over the entries of an [`ArenaSkipMap`] in key order.
pub struct Iter<'a, K, V> {
  next: Ptr<K, V>,
  end: Ptr<K, V>,
  marker: PhantomData<(&'a K, &'a V)>,
}

unsafe impl<K: Sync, V: Sync> Send for Iter<'_, K, V> {}
unsafe impl<K: Sync, V: Sync> Sync for Iter<'_, K, V> {}

impl<K, V> Clone for Iter<'_, K, V> {
  fn clone(&self) -> Self {
    Iter {
      next: self.next,
      end: self.end,
      marker: PhantomData,
    }
  }
}

impl<'a, K, V> Iterator for Iter<'a, K, V> {
  type Item = (&'a K, &'a V);

  #[inline]
  fn next(&mut self) -> Option<(&'a K, &'a V)> {
    if self.next.is_null() || self.next == self.end {
      return None;
    }
    unsafe {
      let n = self.next;
      self.next = next(n, 0);
      Some((&(*n).key, &(*n).value))
    }
  }
}
