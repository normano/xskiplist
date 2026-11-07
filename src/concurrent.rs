// loom's `thread_local!` cannot take `const` initialisers.
#![allow(clippy::missing_const_for_thread_local)]

use crate::node::Levels;
use crate::slab::MAX_HEIGHT;
use crate::sync::{
  AtomicBool, AtomicIsize, AtomicPtr, AtomicU32, AtomicUsize, CAP, MODELED, NodeLock, Ordering, TOWER_CAP,
  compiler_fence, fence, spin_loop, thread_local,
};
use crossbeam_epoch::{self as epoch, Guard};
use std::alloc::{self, Layout};
use std::borrow::Borrow;
use std::cell::Cell;
use std::fmt;
use std::marker::PhantomData;
use std::mem::{MaybeUninit, align_of, size_of};
use std::ops::{Bound, RangeBounds};
use std::ptr;

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
    let h = levels.next().min(TOWER_CAP);
    l.set(Some(levels));
    h
  })
}

#[repr(align(128))]
struct Padded(AtomicIsize);

struct Counter(Box<[Padded; STRIPES]>);

impl Counter {
  fn new() -> Self {
    Counter(Box::new(std::array::from_fn(|_| Padded(AtomicIsize::new(0)))))
  }

  #[inline(always)]
  fn add(&self, n: isize) {
    self.0[stripe()].0.fetch_add(n, Ordering::Relaxed);
  }

  fn sum(&self) -> usize {
    self.0.iter().map(|p| p.0.load(Ordering::Relaxed)).sum::<isize>().max(0) as usize
  }
}

/// One key and value, allocated on its own so that it never moves while a guard can reach it.
struct EntryData<K, V> {
  key: K,
  value: V,
}

type EntryPtr<K, V> = *mut EntryData<K, V>;

/// Node header. `height` atomic forward pointers lie in reverse level order directly before it.
/// `slots[..len]` are the node's entries in key order, written only under `lock` and read under
/// `seq`. `anchor` is a copy of the key the node's range starts at, written before the node is
/// published and never changed; `anchored` false means the node is first and covers everything
/// below its successor. `has_anchor` says whether `anchor` holds a key to drop.
#[repr(C)]
struct Node<K, V> {
  lock: NodeLock,
  marked: AtomicBool,
  linked: AtomicBool,
  anchored: AtomicBool,
  has_anchor: bool,
  height: u8,
  seq: AtomicU32,
  len: AtomicU32,
  anchor: MaybeUninit<K>,
  slots: [AtomicPtr<EntryData<K, V>>; CAP],
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

fn node_layout<K, V>(height: usize) -> (Layout, usize) {
  let align = align_of::<Node<K, V>>().max(align_of::<AtomicPtr<Node<K, V>>>());
  let offset = (size_of::<AtomicPtr<Node<K, V>>>() * height + align - 1) & !(align - 1);
  let layout = Layout::from_size_align(offset + size_of::<Node<K, V>>(), align).expect("node layout");
  (layout, offset)
}

unsafe fn alloc_node<K, V>(height: usize, anchor: Option<K>) -> Ptr<K, V> {
  unsafe {
    let (layout, offset) = node_layout::<K, V>(height);
    let base = alloc::alloc(layout);
    if base.is_null() {
      alloc::handle_alloc_error(layout);
    }
    let node: Ptr<K, V> = base.add(offset).cast();
    for level in 0..height {
      tower_slot(node, level).write(AtomicPtr::new(ptr::null_mut()));
    }
    ptr::addr_of_mut!((*node).lock).write(NodeLock::new());
    ptr::addr_of_mut!((*node).marked).write(AtomicBool::new(false));
    ptr::addr_of_mut!((*node).linked).write(AtomicBool::new(false));
    ptr::addr_of_mut!((*node).anchored).write(AtomicBool::new(anchor.is_some()));
    ptr::addr_of_mut!((*node).has_anchor).write(anchor.is_some());
    ptr::addr_of_mut!((*node).height).write(height as u8);
    ptr::addr_of_mut!((*node).seq).write(AtomicU32::new(0));
    ptr::addr_of_mut!((*node).len).write(AtomicU32::new(0));
    match anchor {
      Some(k) => ptr::addr_of_mut!((*node).anchor).write(MaybeUninit::new(k)),
      None => ptr::addr_of_mut!((*node).anchor).write(MaybeUninit::uninit()),
    }
    let slots = ptr::addr_of_mut!((*node).slots).cast::<AtomicPtr<EntryData<K, V>>>();
    for i in 0..CAP {
      slots.add(i).write(AtomicPtr::new(ptr::null_mut()));
    }
    node
  }
}

/// Drops the anchor key, if any, and frees the node. Entries are the caller's business.
unsafe fn free_node<K, V>(node: Ptr<K, V>) {
  unsafe {
    if (*node).has_anchor {
      ptr::drop_in_place((*node).anchor.as_mut_ptr());
    }
    let (layout, offset) = node_layout::<K, V>((*node).height as usize);
    alloc::dealloc(node.cast::<u8>().sub(offset), layout);
  }
}

#[inline(always)]
unsafe fn height<K, V>(node: Ptr<K, V>) -> usize {
  unsafe { (*node).height as usize }
}

#[inline(always)]
unsafe fn slot<'a, K, V>(node: Ptr<K, V>, i: usize) -> &'a AtomicPtr<EntryData<K, V>> {
  unsafe { (*node).slots.get_unchecked(i) }
}

/// `node.anchor <= key`, with an unanchored node reading as minus infinity.
#[inline(always)]
unsafe fn anchor_le<K, V, Q>(node: Ptr<K, V>, key: &Q) -> bool
where
  K: Borrow<Q>,
  Q: Ord + ?Sized,
{
  unsafe { !(*node).anchored.load(Ordering::Relaxed) || (*node).anchor.assume_init_ref().borrow() <= key }
}

/// `node.anchor < key`, with an unanchored node reading as minus infinity.
#[inline(always)]
unsafe fn anchor_lt<K, V, Q>(node: Ptr<K, V>, key: &Q) -> bool
where
  K: Borrow<Q>,
  Q: Ord + ?Sized,
{
  unsafe { !(*node).anchored.load(Ordering::Relaxed) || (*node).anchor.assume_init_ref().borrow() < key }
}

/// Walks right from `x` at `level` while the next node's anchor is at most `key`. Returns the last
/// node that passed (or `x`) and the first that did not (null at the end). A node's forward
/// pointer is loaded together with its anchor so that one step costs one dependent round trip.
#[inline(always)]
unsafe fn walk<K, V, Q>(x: Ptr<K, V>, level: usize, key: &Q) -> (Ptr<K, V>, Ptr<K, V>)
where
  K: Borrow<Q>,
  Q: Ord + ?Sized,
{
  unsafe {
    let mut x = x;
    let mut n = next(x, level);
    while !n.is_null() {
      let succ = next(n, level);
      if anchor_le(n, key) {
        x = n;
        n = succ;
      } else {
        break;
      }
    }
    (x, n)
  }
}

/// A consistent copy of a node's entry pointers.
struct Snapshot<K, V> {
  len: usize,
  slots: [EntryPtr<K, V>; CAP],
}

impl<K, V> Snapshot<K, V> {
  /// Reads `node` under its seqlock. Entries in the result were in the node after the caller
  /// pinned, so they stay allocated while the guard lives.
  #[inline(always)]
  unsafe fn take(node: Ptr<K, V>) -> Self {
    unsafe {
      loop {
        let seq = (*node).seq.load(Ordering::Acquire);
        if seq & 1 == 1 {
          spin_loop();
          continue;
        }
        let len = (*node).len.load(Ordering::Relaxed) as usize;
        let mut slots = [ptr::null_mut(); CAP];
        for (i, s) in slots.iter_mut().enumerate().take(len.min(CAP)) {
          *s = slot(node, i).load(Ordering::Relaxed);
        }
        fence(Ordering::Acquire);
        if (*node).seq.load(Ordering::Relaxed) == seq {
          return Snapshot {
            len: len.min(CAP),
            slots,
          };
        }
      }
    }
  }

  #[inline(always)]
  fn entries(&self) -> &[EntryPtr<K, V>] {
    &self.slots[..self.len]
  }

  /// Position of the first entry whose key is not less than `key`, and whether it is equal.
  #[inline(always)]
  unsafe fn search<Q>(&self, key: &Q) -> (usize, bool)
  where
    K: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    unsafe {
      let pos = self.entries().partition_point(|e| (**e).key.borrow() < key);
      let found = pos < self.len && (*self.slots[pos]).key.borrow() == key;
      (pos, found)
    }
  }
}

/// Opens the seqlock for writing. The caller holds the node's lock.
#[inline(always)]
unsafe fn begin_write<K, V>(node: Ptr<K, V>) {
  unsafe {
    let seq = (*node).seq.load(Ordering::Relaxed);
    (*node).seq.store(seq | 1, Ordering::Relaxed);
    fence(Ordering::Release);
  }
}

#[inline(always)]
unsafe fn end_write<K, V>(node: Ptr<K, V>) {
  unsafe {
    let seq = (*node).seq.load(Ordering::Relaxed);
    (*node).seq.store(seq.wrapping_add(1), Ordering::Release);
  }
}

/// A concurrent sorted map: lookups and iteration take no locks, writers lock one node and, on a
/// split or a node removal, the node's predecessors. Removed entries are reclaimed through
/// `crossbeam-epoch` once no reader can reach them.
pub struct ConcurrentSkipMap<K, V> {
  head: Ptr<K, V>,
  height: AtomicUsize,
  len: Counter,
  marker: PhantomData<(K, V)>,
}

unsafe impl<K: Send + Sync, V: Send + Sync> Send for ConcurrentSkipMap<K, V> {}
unsafe impl<K: Send + Sync, V: Send + Sync> Sync for ConcurrentSkipMap<K, V> {}

/// A reference to one entry of a [`ConcurrentSkipMap`]. It keeps the entry allocated while it
/// lives, whether or not the entry is still in the map.
pub struct Entry<'a, K, V> {
  entry: EntryPtr<K, V>,
  _guard: Guard,
  marker: PhantomData<&'a ConcurrentSkipMap<K, V>>,
}

unsafe impl<K: Send + Sync, V: Send + Sync> Send for Entry<'_, K, V> {}
unsafe impl<K: Send + Sync, V: Send + Sync> Sync for Entry<'_, K, V> {}

impl<K, V> Entry<'_, K, V> {
  /// The key.
  pub fn key(&self) -> &K {
    unsafe { &(*self.entry).key }
  }

  /// The value.
  pub fn value(&self) -> &V {
    unsafe { &(*self.entry).value }
  }
}

impl<K: fmt::Debug, V: fmt::Debug> fmt::Debug for Entry<'_, K, V> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_tuple("Entry").field(self.key()).field(self.value()).finish()
  }
}

struct Preds<K, V> {
  node: [Ptr<K, V>; MAX_HEIGHT],
  succ: [Ptr<K, V>; MAX_HEIGHT],
}

impl<K, V> ConcurrentSkipMap<K, V> {
  /// Makes a new, empty map.
  pub fn new() -> Self {
    let head = unsafe { alloc_node::<K, V>(TOWER_CAP, None) };
    unsafe { (*head).linked.store(true, Ordering::Relaxed) };
    ConcurrentSkipMap {
      head,
      height: AtomicUsize::new(1),
      len: Counter::new(),
      marker: PhantomData,
    }
  }

  /// Returns the number of entries. Approximate while other threads are writing.
  pub fn len(&self) -> usize {
    self.len.sum()
  }

  /// Returns `true` if the map has no entry.
  pub fn is_empty(&self) -> bool {
    self.len() == 0
  }

  /// Descends to the last node at every level whose anchor is at most `key`, recording the node
  /// and its successor per level.
  #[inline(always)]
  unsafe fn descend<Q>(&self, key: &Q, preds: &mut Preds<K, V>)
  where
    K: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    unsafe {
      let mut x = self.head;
      let top = self.height.load(Ordering::Relaxed);
      for level in top..TOWER_CAP {
        preds.node[level] = self.head;
        preds.succ[level] = next(self.head, level);
      }
      let mut level = top;
      while level > 0 {
        level -= 1;
        let (p, s) = walk(x, level, key);
        x = p;
        preds.node[level] = p;
        preds.succ[level] = s;
      }
    }
  }

  /// The level-0 node whose range holds `key`: the last node with anchor at most `key`, or the
  /// first node when every anchor is greater. Null when the map has no node.
  #[inline(always)]
  unsafe fn locate<Q>(&self, key: &Q) -> Ptr<K, V>
  where
    K: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    unsafe {
      let mut x = self.head;
      let mut level = self.height.load(Ordering::Relaxed);
      while level > 0 {
        level -= 1;
        (x, _) = walk(x, level, key);
      }
      if x == self.head { next(self.head, 0) } else { x }
    }
  }

  /// Returns the entry for `key`.
  pub fn get<Q>(&self, key: &Q) -> Option<Entry<'_, K, V>>
  where
    K: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    let guard = epoch::pin();
    unsafe {
      let mut x = self.locate(key);
      while !x.is_null() {
        let snap = Snapshot::take(x);
        let (pos, found) = snap.search(key);
        if found {
          return Some(Entry {
            entry: snap.slots[pos],
            _guard: guard,
            marker: PhantomData,
          });
        }
        let n = next(x, 0);
        if n.is_null() || !anchor_le(n, key) {
          return None;
        }
        x = n;
      }
      None
    }
  }

  /// Returns `true` if `key` is present.
  pub fn contains_key<Q>(&self, key: &Q) -> bool
  where
    K: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    self.get(key).is_some()
  }

  /// Locks the node whose range holds `key` and returns it, or null when the map is empty. The
  /// node is not marked and its successor's anchor is above `key`. Every retry yields first: a
  /// node stays marked or split only until the thread holding it finishes, and the model checkers
  /// treat a retry without a yield as a spin.
  #[inline(always)]
  unsafe fn lock_owner<Q>(&self, key: &Q, preds: &mut Preds<K, V>) -> Ptr<K, V>
  where
    K: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    unsafe {
      loop {
        self.descend(key, preds);
        let mut x = preds.node[0];
        if x == self.head {
          x = next(self.head, 0);
          if x.is_null() {
            return x;
          }
        }
        (*x).lock.lock();
        if (*x).marked.load(Ordering::Relaxed) {
          (*x).lock.unlock();
          spin_loop();
          continue;
        }
        let n = next(x, 0);
        if !n.is_null() && anchor_le(n, key) {
          (*x).lock.unlock();
          spin_loop();
          continue;
        }
        return x;
      }
    }
  }

  /// Links a first node holding `entry` into an empty map. Returns `false` if the map was no
  /// longer empty.
  unsafe fn link_first(&self, entry: EntryPtr<K, V>) -> bool {
    unsafe {
      let head = self.head;
      (*head).lock.lock();
      if !next(head, 0).is_null() {
        (*head).lock.unlock();
        return false;
      }
      let h = next_height();
      let node = alloc_node::<K, V>(h, None);
      slot(node, 0).store(entry, Ordering::Relaxed);
      (*node).len.store(1, Ordering::Relaxed);
      for level in 0..h {
        tower(head, level).store(node, Ordering::Release);
      }
      (*node).linked.store(true, Ordering::Release);
      self.height.fetch_max(h, Ordering::Relaxed);
      (*head).lock.unlock();
      true
    }
  }

  /// Puts `entry` into the locked node `x` at `pos`, splitting `x` first when it is full. Unlocks
  /// `x` before returning. On a split the new node is linked before `x`'s seqlock closes, so a
  /// reader that validates a snapshot of the shortened `x` also sees its new successor; a reader
  /// with an older snapshot may see moved entries twice, which the walker's skip rule absorbs.
  unsafe fn place(&self, x: Ptr<K, V>, pos: usize, entry: EntryPtr<K, V>, preds: &Preds<K, V>) -> bool
  where
    K: Clone,
  {
    unsafe {
      let len = (*x).len.load(Ordering::Relaxed) as usize;
      if len < CAP {
        begin_write(x);
        for i in (pos..len).rev() {
          slot(x, i + 1).store(slot(x, i).load(Ordering::Relaxed), Ordering::Relaxed);
        }
        slot(x, pos).store(entry, Ordering::Relaxed);
        (*x).len.store((len + 1) as u32, Ordering::Relaxed);
        end_write(x);
        (*x).lock.unlock();
        return true;
      }
      if !(*x).linked.load(Ordering::Acquire) {
        (*x).lock.unlock();
        return false;
      }
      let xh = height(x);
      let h = next_height();
      let mut locked: [Ptr<K, V>; MAX_HEIGHT] = [ptr::null_mut(); MAX_HEIGHT];
      let mut count = 0;
      let mut valid = true;
      for level in xh..h {
        let p = preds.node[level];
        if count > 0 && locked[count - 1] == p {
          continue;
        }
        (*p).lock.lock();
        locked[count] = p;
        count += 1;
        if (*p).marked.load(Ordering::Relaxed) {
          valid = false;
          break;
        }
      }
      if valid {
        for level in xh..h {
          if next(preds.node[level], level) != preds.succ[level] {
            valid = false;
            break;
          }
        }
      }
      if !valid {
        for &p in locked.iter().take(count) {
          (*p).lock.unlock();
        }
        (*x).lock.unlock();
        return false;
      }
      let half = CAP / 2;
      let first = next(self.head, 0) == x;
      let n = alloc_node::<K, V>(h, Some((*slot(x, half).load(Ordering::Relaxed)).key.clone()));
      for i in half..CAP {
        slot(n, i - half).store(slot(x, i).load(Ordering::Relaxed), Ordering::Relaxed);
      }
      let mut nlen = CAP - half;
      if pos > half {
        let p = pos - half;
        for i in (p..nlen).rev() {
          slot(n, i + 1).store(slot(n, i).load(Ordering::Relaxed), Ordering::Relaxed);
        }
        slot(n, p).store(entry, Ordering::Relaxed);
        nlen += 1;
      }
      (*n).len.store(nlen as u32, Ordering::Relaxed);
      begin_write(x);
      let mut xlen = half;
      if pos <= half {
        for i in (pos..half).rev() {
          slot(x, i + 1).store(slot(x, i).load(Ordering::Relaxed), Ordering::Relaxed);
        }
        slot(x, pos).store(entry, Ordering::Relaxed);
        xlen += 1;
      }
      (*x).len.store(xlen as u32, Ordering::Relaxed);
      if first {
        (*x).anchored.store(false, Ordering::Release);
      }
      for level in 0..h {
        let succ = if level < xh { next(x, level) } else { preds.succ[level] };
        tower(n, level).store(succ, Ordering::Relaxed);
      }
      for level in 0..h {
        let pred = if level < xh { x } else { preds.node[level] };
        tower(pred, level).store(n, Ordering::Release);
      }
      (*n).linked.store(true, Ordering::Release);
      end_write(x);
      self.height.fetch_max(h, Ordering::Relaxed);
      for &p in locked.iter().take(count) {
        (*p).lock.unlock();
      }
      (*x).lock.unlock();
      true
    }
  }

  /// Inserts `key` with `value`, replacing the value of a present key, and returns the entry.
  pub fn insert(&self, key: K, value: V) -> Entry<'_, K, V>
  where
    K: Ord + Clone,
  {
    let guard = epoch::pin();
    let entry = Box::into_raw(Box::new(EntryData { key, value }));
    unsafe {
      let mut preds = Preds {
        node: [ptr::null_mut(); MAX_HEIGHT],
        succ: [ptr::null_mut(); MAX_HEIGHT],
      };
      loop {
        let key = &(*entry).key;
        let x = self.lock_owner(key, &mut preds);
        if x.is_null() {
          if self.link_first(entry) {
            self.len.add(1);
            break;
          }
          continue;
        }
        let snap = Snapshot::take(x);
        let (pos, found) = snap.search(key);
        if found {
          let old = snap.slots[pos];
          begin_write(x);
          slot(x, pos).store(entry, Ordering::Relaxed);
          end_write(x);
          (*x).lock.unlock();
          guard.defer_unchecked(move || drop(Box::from_raw(old)));
          break;
        }
        if self.place(x, pos, entry, &preds) {
          self.len.add(1);
          break;
        }
        spin_loop();
      }
    }
    Entry {
      entry,
      _guard: guard,
      marker: PhantomData,
    }
  }

  /// Returns the entry for `key`, inserting `value` first if the key is absent.
  pub fn get_or_insert(&self, key: K, value: V) -> Entry<'_, K, V>
  where
    K: Ord + Clone,
  {
    let guard = epoch::pin();
    let entry = Box::into_raw(Box::new(EntryData { key, value }));
    unsafe {
      let mut preds = Preds {
        node: [ptr::null_mut(); MAX_HEIGHT],
        succ: [ptr::null_mut(); MAX_HEIGHT],
      };
      loop {
        let key = &(*entry).key;
        let x = self.lock_owner(key, &mut preds);
        if x.is_null() {
          if self.link_first(entry) {
            self.len.add(1);
            break;
          }
          continue;
        }
        let snap = Snapshot::take(x);
        let (pos, found) = snap.search(key);
        if found {
          (*x).lock.unlock();
          drop(Box::from_raw(entry));
          return Entry {
            entry: snap.slots[pos],
            _guard: guard,
            marker: PhantomData,
          };
        }
        if self.place(x, pos, entry, &preds) {
          self.len.add(1);
          break;
        }
        spin_loop();
      }
    }
    Entry {
      entry,
      _guard: guard,
      marker: PhantomData,
    }
  }

  /// Removes `key` and returns its entry.
  pub fn remove<Q>(&self, key: &Q) -> Option<Entry<'_, K, V>>
  where
    K: Borrow<Q> + Ord,
    Q: Ord + ?Sized,
  {
    let guard = epoch::pin();
    unsafe {
      let mut preds = Preds {
        node: [ptr::null_mut(); MAX_HEIGHT],
        succ: [ptr::null_mut(); MAX_HEIGHT],
      };
      let x = self.lock_owner(key, &mut preds);
      if x.is_null() {
        return None;
      }
      let snap = Snapshot::take(x);
      let (pos, found) = snap.search(key);
      if !found {
        (*x).lock.unlock();
        return None;
      }
      let entry = snap.slots[pos];
      let len = snap.len;
      begin_write(x);
      for i in pos + 1..len {
        slot(x, i - 1).store(slot(x, i).load(Ordering::Relaxed), Ordering::Relaxed);
      }
      (*x).len.store((len - 1) as u32, Ordering::Relaxed);
      end_write(x);
      self.len.add(-1);
      if len == 1 {
        self.unlink(x, &guard);
      } else {
        (*x).lock.unlock();
      }
      guard.defer_unchecked(move || drop(Box::from_raw(entry)));
      Some(Entry {
        entry,
        _guard: guard,
        marker: PhantomData,
      })
    }
  }

  /// Unlinks the empty, locked node `x` from every level and frees it once no reader can reach
  /// it. Unlocks `x`.
  unsafe fn unlink(&self, x: Ptr<K, V>, guard: &Guard)
  where
    K: Ord,
  {
    unsafe {
      (*x).marked.store(true, Ordering::Relaxed);
      while !(*x).linked.load(Ordering::Acquire) {
        spin_loop();
      }
      let xh = height(x);
      let anchored = (*x).anchored.load(Ordering::Relaxed);
      let a: *const K = (*x).anchor.as_ptr();
      let mut preds: [Ptr<K, V>; MAX_HEIGHT] = [ptr::null_mut(); MAX_HEIGHT];
      loop {
        let mut p = self.head;
        let mut level = self.height.load(Ordering::Relaxed).max(xh);
        while level > 0 {
          level -= 1;
          let mut n = next(p, level);
          while !n.is_null() && n != x && anchored {
            let succ = next(n, level);
            if !anchor_lt(n, &*a) {
              break;
            }
            p = n;
            n = succ;
          }
          preds[level] = p;
        }
        let mut locked: [Ptr<K, V>; MAX_HEIGHT] = [ptr::null_mut(); MAX_HEIGHT];
        let mut count = 0;
        let mut valid = true;
        for &p in preds.iter().take(xh) {
          if count > 0 && locked[count - 1] == p {
            continue;
          }
          (*p).lock.lock();
          locked[count] = p;
          count += 1;
          if (*p).marked.load(Ordering::Relaxed) {
            valid = false;
            break;
          }
        }
        if valid {
          for (level, &p) in preds.iter().enumerate().take(xh) {
            if next(p, level) != x {
              valid = false;
              break;
            }
          }
        }
        if valid {
          for level in (0..xh).rev() {
            tower(preds[level], level).store(next(x, level), Ordering::Release);
          }
        }
        for &p in locked.iter().take(count) {
          (*p).lock.unlock();
        }
        if valid {
          break;
        }
        spin_loop();
      }
      (*x).lock.unlock();
      guard.defer_unchecked(move || free_node(x));
    }
  }

  /// The smallest entry.
  pub fn front(&self) -> Option<Entry<'_, K, V>> {
    let guard = epoch::pin();
    unsafe {
      let mut x = next(self.head, 0);
      while !x.is_null() {
        let snap = Snapshot::take(x);
        if snap.len > 0 {
          return Some(Entry {
            entry: snap.slots[0],
            _guard: guard,
            marker: PhantomData,
          });
        }
        x = next(x, 0);
      }
      None
    }
  }

  /// The largest entry.
  pub fn back(&self) -> Option<Entry<'_, K, V>> {
    let guard = epoch::pin();
    unsafe {
      let mut x = self.head;
      let mut level = self.height.load(Ordering::Relaxed);
      while level > 0 {
        level -= 1;
        let mut n = next(x, level);
        while !n.is_null() {
          x = n;
          n = next(x, level);
        }
      }
      if x == self.head {
        return None;
      }
      let snap = Snapshot::take(x);
      if snap.len == 0 {
        return None;
      }
      Some(Entry {
        entry: snap.slots[snap.len - 1],
        _guard: guard,
        marker: PhantomData,
      })
    }
  }

  /// Iterates over the entries in key order. Entries inserted or removed during the iteration
  /// may or may not be visited.
  pub fn iter(&self) -> Iter<'_, K, V> {
    let guard = epoch::pin();
    let walker = unsafe { Walker::from_node(next(self.head, 0), 0) };
    Iter {
      walker,
      _guard: guard,
      marker: PhantomData,
    }
  }

  /// Iterates over the entries whose keys lie in `range`.
  pub fn range<Q, R>(&self, range: R) -> Range<'_, Q, R, K, V>
  where
    K: Borrow<Q>,
    Q: Ord + ?Sized,
    R: RangeBounds<Q>,
  {
    let guard = epoch::pin();
    let walker = unsafe {
      match range.start_bound() {
        Bound::Unbounded => Walker::from_node(next(self.head, 0), 0),
        Bound::Included(k) => {
          let x = self.locate(k);
          let mut w = Walker::from_node(x, 0);
          w.pos = w.snap.entries().partition_point(|e| (**e).key.borrow() < k);
          w
        }
        Bound::Excluded(k) => {
          let x = self.locate(k);
          let mut w = Walker::from_node(x, 0);
          w.pos = w.snap.entries().partition_point(|e| (**e).key.borrow() <= k);
          w
        }
      }
    };
    Range {
      walker,
      range,
      _guard: guard,
      marker: PhantomData,
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
      let mut prev: Option<EntryPtr<K, V>> = None;
      let mut nodes = Vec::new();
      let mut first = true;
      while !x.is_null() {
        assert!(!(*x).marked.load(Ordering::Relaxed), "marked node reachable");
        let snap = Snapshot::take(x);
        assert!(snap.len >= 1, "empty node reachable");
        let anchored = (*x).anchored.load(Ordering::Relaxed);
        if !first {
          assert!(anchored, "unanchored later node");
        }
        for &e in snap.entries() {
          if let Some(p) = prev {
            assert!((*p).key < (*e).key, "keys out of order");
          }
          if anchored && !first {
            assert!(*(*x).anchor.assume_init_ref() <= (*e).key, "entry below anchor");
          }
          prev = Some(e);
          count += 1;
        }
        nodes.push((x, height(x)));
        first = false;
        x = next(x, 0);
      }
      assert_eq!(count, self.len(), "len");
      for level in 1..TOWER_CAP {
        let expected: Vec<Ptr<K, V>> = nodes.iter().filter(|(_, h)| *h > level).map(|(n, _)| *n).collect();
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

impl<K, V> Default for ConcurrentSkipMap<K, V> {
  fn default() -> Self {
    ConcurrentSkipMap::new()
  }
}

impl<K, V> Drop for ConcurrentSkipMap<K, V> {
  fn drop(&mut self) {
    unsafe {
      let mut x = next(self.head, 0);
      while !x.is_null() {
        let after = next(x, 0);
        let len = (*x).len.load(Ordering::Relaxed) as usize;
        for i in 0..len {
          drop(Box::from_raw(slot(x, i).load(Ordering::Relaxed)));
        }
        free_node(x);
        x = after;
      }
      free_node(self.head);
    }
  }
}

impl<K: Ord + Clone, V> FromIterator<(K, V)> for ConcurrentSkipMap<K, V> {
  fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
    let map = ConcurrentSkipMap::new();
    for (k, v) in iter {
      map.insert(k, v);
    }
    map
  }
}

impl<K: fmt::Debug + Ord, V: fmt::Debug> fmt::Debug for ConcurrentSkipMap<K, V> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let mut m = f.debug_map();
    for e in self.iter() {
      m.entry(e.key(), e.value());
    }
    m.finish()
  }
}

/// Walks level 0 through node snapshots, yielding each entry once even when a split moves entries
/// into a node not yet visited. A split can also move entries below a range's start into the next
/// node before anything was yielded, so `Range` re-checks its start bound on every entry.
struct Walker<K, V> {
  node: Ptr<K, V>,
  snap: Snapshot<K, V>,
  pos: usize,
  last: EntryPtr<K, V>,
}

impl<K, V> Walker<K, V> {
  unsafe fn from_node(node: Ptr<K, V>, pos: usize) -> Self {
    let snap = if node.is_null() {
      Snapshot {
        len: 0,
        slots: [ptr::null_mut(); CAP],
      }
    } else {
      unsafe { Snapshot::take(node) }
    };
    Walker {
      node,
      snap,
      pos,
      last: ptr::null_mut(),
    }
  }

  /// The next entry, or null at the end.
  unsafe fn advance(&mut self) -> EntryPtr<K, V>
  where
    K: Ord,
  {
    unsafe {
      loop {
        if self.node.is_null() {
          return ptr::null_mut();
        }
        if self.pos < self.snap.len {
          let e = self.snap.slots[self.pos];
          self.pos += 1;
          self.last = e;
          return e;
        }
        self.node = next(self.node, 0);
        if self.node.is_null() {
          return ptr::null_mut();
        }
        self.snap = Snapshot::take(self.node);
        let last = self.last;
        self.pos = if last.is_null() {
          0
        } else {
          self.snap.entries().partition_point(|e| (**e).key <= (*last).key)
        };
      }
    }
  }

  fn stop(&mut self) {
    self.node = ptr::null_mut();
  }
}

/// Iterator over the entries of a [`ConcurrentSkipMap`] in key order.
pub struct Iter<'a, K, V> {
  walker: Walker<K, V>,
  _guard: Guard,
  marker: PhantomData<&'a ConcurrentSkipMap<K, V>>,
}

unsafe impl<K: Send + Sync, V: Send + Sync> Send for Iter<'_, K, V> {}

impl<'a, K: Ord, V> Iterator for Iter<'a, K, V> {
  type Item = Entry<'a, K, V>;

  fn next(&mut self) -> Option<Entry<'a, K, V>> {
    let e = unsafe { self.walker.advance() };
    if e.is_null() {
      return None;
    }
    Some(Entry {
      entry: e,
      _guard: epoch::pin(),
      marker: PhantomData,
    })
  }
}

/// Iterator over the entries of a [`ConcurrentSkipMap`] within a key range.
pub struct Range<'a, Q: ?Sized, R, K, V> {
  walker: Walker<K, V>,
  range: R,
  _guard: Guard,
  marker: RangeMarker<'a, Q, K, V>,
}

type RangeMarker<'a, Q, K, V> = PhantomData<(&'a ConcurrentSkipMap<K, V>, fn() -> &'a Q)>;

unsafe impl<Q: ?Sized, R: Send, K: Send + Sync, V: Send + Sync> Send for Range<'_, Q, R, K, V> {}

impl<'a, Q, R, K, V> Iterator for Range<'a, Q, R, K, V>
where
  K: Borrow<Q> + Ord,
  Q: Ord + ?Sized,
  R: RangeBounds<Q>,
{
  type Item = Entry<'a, K, V>;

  fn next(&mut self) -> Option<Entry<'a, K, V>> {
    loop {
      let e = unsafe { self.walker.advance() };
      if e.is_null() {
        return None;
      }
      let key = unsafe { (*e).key.borrow() };
      let below = match self.range.start_bound() {
        Bound::Unbounded => false,
        Bound::Included(k) => key < k,
        Bound::Excluded(k) => key <= k,
      };
      if below {
        continue;
      }
      let past = match self.range.end_bound() {
        Bound::Unbounded => false,
        Bound::Included(k) => key > k,
        Bound::Excluded(k) => key >= k,
      };
      if past {
        self.walker.stop();
        return None;
      }
      return Some(Entry {
        entry: e,
        _guard: epoch::pin(),
        marker: PhantomData,
      });
    }
  }
}
