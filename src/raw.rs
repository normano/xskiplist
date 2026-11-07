use crate::node::{
  Levels, Link, Node, alloc_head, alloc_node, cap, drop_entries, drop_entry, free_head, free_node, height, key_slice,
  keys, len, link, move_entries, next, read_entry, set_len, shift_entries, slab_of, vals, write_entry,
};
use crate::slab::{Family, MAX_HEIGHT};
use std::cmp::Ordering;
use std::marker::PhantomData;
use std::mem::{self, ManuallyDrop, MaybeUninit};
use std::ptr::{self, NonNull};
use std::sync::Arc;

pub(crate) type Ptr<K, V> = *mut Node<K, V>;

/// One entry: a node and a slot in it. `node` is null for no entry.
pub(crate) struct Cursor<K, V> {
  pub(crate) node: *const Node<K, V>,
  pub(crate) slot: usize,
}

impl<K, V> Clone for Cursor<K, V> {
  fn clone(&self) -> Self {
    *self
  }
}

impl<K, V> Copy for Cursor<K, V> {}

impl<K, V> Cursor<K, V> {
  pub(crate) const NONE: Self = Cursor {
    node: ptr::null(),
    slot: 0,
  };

  #[inline(always)]
  pub(crate) fn is_null(&self) -> bool {
    self.node.is_null()
  }

  #[inline(always)]
  pub(crate) unsafe fn key<'a>(self) -> &'a K {
    unsafe { &*keys(self.node.cast_mut()).add(self.slot) }
  }

  #[inline(always)]
  pub(crate) unsafe fn key_mut<'a>(self) -> &'a mut K {
    unsafe { &mut *keys(self.node.cast_mut()).add(self.slot) }
  }

  #[inline(always)]
  pub(crate) unsafe fn value<'a>(self) -> &'a V {
    unsafe { &*vals(self.node.cast_mut()).add(self.slot) }
  }

  #[inline(always)]
  pub(crate) unsafe fn value_mut<'a>(self) -> &'a mut V {
    unsafe { &mut *vals(self.node.cast_mut()).add(self.slot) }
  }

  #[inline(always)]
  pub(crate) unsafe fn pair<'a>(self) -> Option<(&'a K, &'a V)> {
    if self.is_null() {
      None
    } else {
      unsafe { Some((self.key(), self.value())) }
    }
  }

  #[inline(always)]
  pub(crate) unsafe fn pair_mut<'a>(self) -> Option<(&'a K, &'a mut V)> {
    if self.is_null() {
      None
    } else {
      unsafe { Some((self.key(), self.value_mut())) }
    }
  }
}

/// Where a descent landed: on each level below `height`, the last node whose first key passed the
/// walk's test (the head if none) and the index of that node's first entry, plus the slot inside
/// the level-0 node where the target entry is or would go. Levels at or above `height` read as the
/// head at index 0.
pub(crate) struct Path<K, V> {
  head: Ptr<K, V>,
  height: usize,
  node: [MaybeUninit<Ptr<K, V>>; MAX_HEIGHT],
  start: [MaybeUninit<usize>; MAX_HEIGHT],
  pub(crate) slot: usize,
}

impl<K, V> Path<K, V> {
  #[inline(always)]
  fn new(head: Ptr<K, V>, height: usize) -> Self {
    Path {
      head,
      height,
      node: [MaybeUninit::uninit(); MAX_HEIGHT],
      start: [MaybeUninit::uninit(); MAX_HEIGHT],
      slot: 0,
    }
  }

  #[inline(always)]
  unsafe fn set(&mut self, level: usize, node: Ptr<K, V>, start: usize) {
    unsafe {
      self.node.get_unchecked_mut(level).write(node);
      self.start.get_unchecked_mut(level).write(start);
    }
  }

  /// Makes levels `self.height..height` explicit as the head at index 0.
  #[inline(always)]
  unsafe fn raise(&mut self, height: usize) {
    for level in self.height..height {
      unsafe { self.set(level, self.head, 0) }
    }
    if height > self.height {
      self.height = height;
    }
  }

  /// The node landed on at `level` and the index of its first entry.
  #[inline(always)]
  pub(crate) fn pred(&self, level: usize) -> (Ptr<K, V>, usize) {
    if level < self.height {
      unsafe {
        (
          self.node.get_unchecked(level).assume_init(),
          self.start.get_unchecked(level).assume_init(),
        )
      }
    } else {
      (self.head, 0)
    }
  }

  /// The level-0 node.
  #[inline(always)]
  pub(crate) fn node(&self) -> Ptr<K, V> {
    self.pred(0).0
  }

  /// The index of the level-0 node's first entry.
  #[inline(always)]
  pub(crate) fn start(&self) -> usize {
    self.pred(0).1
  }

  /// The index of the target entry.
  #[inline(always)]
  pub(crate) fn index(&self) -> usize {
    self.start() + self.slot
  }

  /// Moves the path onto the next node when the slot is past the end of the level-0 node, so that
  /// the target entry, if there is one, is at `(node(), slot)`.
  #[inline(always)]
  pub(crate) unsafe fn normalize(&mut self) {
    unsafe {
      let x = self.node();
      let own = if x == self.head { 0 } else { len(x) };
      if x != self.head && self.slot < own {
        return;
      }
      let n = next(x, 0);
      if n.is_null() {
        return;
      }
      let start = self.start() + own;
      for level in 0..height(n) {
        self.set(level, n, start);
      }
      self.slot = 0;
    }
  }

  /// The target entry after [`normalize`](Self::normalize), or none when the path points past the
  /// end.
  #[inline(always)]
  pub(crate) unsafe fn target(&self) -> Cursor<K, V> {
    let x = self.node();
    if x == self.head || self.slot >= unsafe { len(x) } {
      Cursor::NONE
    } else {
      Cursor {
        node: x,
        slot: self.slot,
      }
    }
  }
}

/// An unrolled skiplist with link widths, shared by every collection in the crate. Ordering is
/// the caller's business: every operation here is by index or by a monotone predicate over keys.
pub(crate) struct RawList<K, V> {
  head: NonNull<Node<K, V>>,
  tail: *const Node<K, V>,
  len: usize,
  height: usize,
  levels: Levels,
  marker: PhantomData<Box<Node<K, V>>>,
}

unsafe impl<K: Send, V: Send> Send for RawList<K, V> {}
unsafe impl<K: Sync, V: Sync> Sync for RawList<K, V> {}

#[inline(always)]
fn partition<K>(keys: &[K], mut pass: impl FnMut(&K) -> bool) -> usize {
  keys.partition_point(|k| pass(k))
}

impl<K, V> RawList<K, V> {
  pub(crate) fn new() -> Self {
    RawList::with(Arc::default(), Levels::new())
  }

  fn with(family: Arc<Family>, levels: Levels) -> Self {
    RawList {
      head: alloc_head(family),
      tail: ptr::null(),
      len: 0,
      height: 0,
      levels,
      marker: PhantomData,
    }
  }

  #[inline(always)]
  pub(crate) fn len(&self) -> usize {
    self.len
  }

  #[inline(always)]
  pub(crate) fn head(&self) -> Ptr<K, V> {
    self.head.as_ptr()
  }

  #[inline(always)]
  pub(crate) fn first(&self) -> Cursor<K, V> {
    let node = unsafe { next(self.head(), 0) };
    Cursor { node, slot: 0 }
  }

  #[inline(always)]
  pub(crate) fn last(&self) -> Cursor<K, V> {
    if self.tail.is_null() {
      Cursor::NONE
    } else {
      Cursor {
        node: self.tail,
        slot: unsafe { len(self.tail.cast_mut()) - 1 },
      }
    }
  }

  /// The entry before `c`, or the last entry when `c` is none.
  #[inline(always)]
  pub(crate) unsafe fn before(&self, c: Cursor<K, V>) -> Cursor<K, V> {
    if c.is_null() {
      return self.last();
    }
    if c.slot > 0 {
      return Cursor {
        node: c.node,
        slot: c.slot - 1,
      };
    }
    unsafe {
      let p = (*c.node).prev;
      if p.is_null() {
        Cursor::NONE
      } else {
        Cursor {
          node: p,
          slot: len(p.cast_mut()) - 1,
        }
      }
    }
  }

  /// An unfilled path for this list, to hand to [`path_where`](Self::path_where),
  /// [`search`](Self::search) or [`path_to`](Self::path_to). The caller owns it so that the record
  /// is never moved.
  #[inline(always)]
  pub(crate) fn path(&self) -> Path<K, V> {
    Path::new(self.head(), self.height)
  }

  /// Node-level walk: lands on the last node whose first key passes `go_right` (the head if none)
  /// at every level and records it.
  #[inline(always)]
  unsafe fn walk(&self, mut go_right: impl FnMut(&K) -> bool, path: &mut Path<K, V>) -> Ptr<K, V> {
    unsafe {
      let mut x = self.head();
      let mut start = 0;
      let mut bound: Ptr<K, V> = ptr::null_mut();
      let mut level = self.height;
      while level > 0 {
        level -= 1;
        loop {
          let l = *link(x, level);
          if l.next == bound {
            break;
          }
          if go_right(&*keys(l.next)) {
            start += l.width;
            x = l.next;
          } else {
            bound = l.next;
            break;
          }
        }
        path.set(level, x, start);
      }
      x
    }
  }

  /// [`walk`](Self::walk) without the record, returning the node and the index of its first entry.
  #[inline(always)]
  unsafe fn walk_to(&self, mut go_right: impl FnMut(&K) -> bool) -> (Ptr<K, V>, usize) {
    unsafe {
      let mut x = self.head();
      let mut start = 0;
      let mut bound: Ptr<K, V> = ptr::null_mut();
      let mut level = self.height;
      while level > 0 {
        level -= 1;
        loop {
          let l = *link(x, level);
          if l.next == bound {
            break;
          }
          if go_right(&*keys(l.next)) {
            start += l.width;
            x = l.next;
          } else {
            bound = l.next;
            break;
          }
        }
      }
      (x, start)
    }
  }

  /// Fills `path` for the first entry whose key fails `go_right`, which must be monotone over the
  /// list. The slot may equal the node's length, in which case the entry is the next node's first
  /// or there is none.
  #[inline(always)]
  pub(crate) unsafe fn path_where(&self, mut go_right: impl FnMut(&K) -> bool, path: &mut Path<K, V>) {
    unsafe {
      let x = self.walk(&mut go_right, path);
      path.slot = if x == self.head() {
        0
      } else {
        partition(key_slice(x), go_right)
      };
    }
  }

  /// Fills `path` for the key that `cmp` compares equal to. Returns the entry when it is present;
  /// otherwise the path points where it would be inserted.
  #[inline(always)]
  pub(crate) unsafe fn search(
    &self,
    mut cmp: impl FnMut(&K) -> Ordering,
    path: &mut Path<K, V>,
  ) -> Option<Cursor<K, V>> {
    unsafe {
      let x = self.walk(|k| cmp(k) != Ordering::Greater, path);
      if x == self.head() {
        path.slot = 0;
        return None;
      }
      match key_slice(x).binary_search_by(&mut cmp) {
        Ok(slot) => {
          path.slot = slot;
          Some(Cursor { node: x, slot })
        }
        Err(slot) => {
          path.slot = slot;
          None
        }
      }
    }
  }

  /// The entry that `cmp` compares equal to, or none.
  #[inline(always)]
  pub(crate) unsafe fn find(&self, mut cmp: impl FnMut(&K) -> Ordering) -> Cursor<K, V> {
    unsafe {
      let (x, _) = self.walk_to(|k| cmp(k) != Ordering::Greater);
      if x == self.head() {
        return Cursor::NONE;
      }
      match key_slice(x).binary_search_by(&mut cmp) {
        Ok(slot) => Cursor { node: x, slot },
        Err(_) => Cursor::NONE,
      }
    }
  }

  /// The first entry whose key fails `go_right` (none if every key passes) and its index, which is
  /// the number of entries that pass.
  #[inline(always)]
  pub(crate) unsafe fn lower_bound(&self, mut go_right: impl FnMut(&K) -> bool) -> (Cursor<K, V>, usize) {
    unsafe {
      let (x, start) = self.walk_to(&mut go_right);
      if x == self.head() {
        return (self.first(), 0);
      }
      let slot = partition(key_slice(x), go_right);
      let index = start + slot;
      if slot < len(x) {
        (Cursor { node: x, slot }, index)
      } else {
        (
          Cursor {
            node: next(x, 0),
            slot: 0,
          },
          index,
        )
      }
    }
  }

  /// Fills `path` for the entry at `index`, which must be at most `len`. At `len` the path points
  /// past the last entry of the last node.
  #[inline(always)]
  pub(crate) unsafe fn path_to(&self, index: usize, path: &mut Path<K, V>) {
    unsafe {
      let mut x = self.head();
      let mut start = 0;
      let mut level = self.height;
      while level > 0 {
        level -= 1;
        loop {
          let l = *link(x, level);
          if l.next.is_null() || start + l.width > index {
            break;
          }
          start += l.width;
          x = l.next;
        }
        path.set(level, x, start);
      }
      path.slot = index - start;
    }
  }

  /// The entry at `index`, which must be below `len`.
  #[inline(always)]
  pub(crate) unsafe fn node_at(&self, index: usize) -> Cursor<K, V> {
    unsafe {
      let mut x = self.head();
      let mut start = 0;
      let mut level = self.height;
      while level > 0 {
        level -= 1;
        loop {
          let l = *link(x, level);
          if l.next.is_null() || start + l.width > index {
            break;
          }
          start += l.width;
          x = l.next;
        }
      }
      Cursor {
        node: x,
        slot: index - start,
      }
    }
  }

  /// The nodes strictly before `x` (whose first entry is at `start`) on every level, with the index
  /// of each one's first entry. `x` must hold at least one entry.
  #[inline(always)]
  unsafe fn preds_of(&self, x: Ptr<K, V>, start: usize, preds: &mut [(Ptr<K, V>, usize); MAX_HEIGHT]) {
    unsafe {
      let mut p = self.head();
      let mut s = 0;
      let mut level = self.height;
      while level > 0 {
        level -= 1;
        loop {
          let l = *link(p, level);
          if l.next.is_null() || l.next == x || s + l.width > start {
            break;
          }
          s += l.width;
          p = l.next;
        }
        preds[level] = (p, s);
      }
    }
  }

  /// Splits `x` so that its entries from `slot` on move to a new node right after it. Returns the
  /// new node. `path` must have landed on `x` and is raised to the new node's height.
  #[inline(always)]
  unsafe fn split_node(&mut self, x: Ptr<K, V>, slot: usize, path: &mut Path<K, V>) -> Ptr<K, V> {
    unsafe {
      let head = self.head();
      let xh = height(x);
      let xlen = len(x);
      let h = self.levels.next();
      let n = alloc_node(head, h, x);
      move_entries(x, slot, n, 0, xlen - slot);
      set_len(n, xlen - slot);
      set_len(x, slot);
      let start_x = path.start();
      let start_n = start_x + slot;
      if h > self.height {
        path.raise(h);
        self.height = h;
      }
      for level in 0..h {
        if level < xh {
          let old = *link(x, level);
          *link(n, level) = if old.next.is_null() {
            Link::EMPTY
          } else {
            Link {
              next: old.next,
              width: old.width - slot,
            }
          };
          *link(x, level) = Link { next: n, width: slot };
        } else {
          let (pred, pstart) = path.pred(level);
          let pl = link(pred, level);
          let old = *pl;
          let before = start_n - pstart;
          *link(n, level) = if old.next.is_null() {
            Link::EMPTY
          } else {
            Link {
              next: old.next,
              width: old.width - before,
            }
          };
          *pl = Link { next: n, width: before };
        }
      }
      let after = next(n, 0);
      if after.is_null() {
        self.tail = n;
      } else {
        (*after).prev = n;
      }
      n
    }
  }

  /// Inserts an entry at the path's target and returns it.
  #[inline(always)]
  pub(crate) unsafe fn insert_at(&mut self, path: &mut Path<K, V>, key: K, value: V) -> Cursor<K, V> {
    unsafe {
      let head = self.head();
      let mut x = path.node();
      let mut slot = path.slot;
      if x == head {
        let first = next(head, 0);
        if first.is_null() {
          let h = self.levels.next();
          let node = alloc_node(head, h, ptr::null_mut());
          write_entry(node, 0, key, value);
          set_len(node, 1);
          for level in 0..h {
            *link(node, level) = Link::EMPTY;
            *link(head, level) = Link { next: node, width: 0 };
          }
          self.height = h;
          self.tail = node;
          self.len = 1;
          return Cursor { node, slot: 0 };
        }
        x = first;
        slot = 0;
        for level in 0..height(x) {
          path.set(level, x, 0);
        }
      }
      if len(x) == cap::<K, V>() {
        let half = cap::<K, V>() / 2;
        let n = self.split_node(x, half, path);
        if slot > half {
          let start_n = path.start() + half;
          x = n;
          slot -= half;
          for level in 0..height(n) {
            path.set(level, n, start_n);
          }
        }
      }
      let xlen = len(x);
      shift_entries(x, slot, slot + 1, xlen - slot);
      write_entry(x, slot, key, value);
      set_len(x, xlen + 1);
      for level in 0..self.height {
        let pl = link(path.pred(level).0, level);
        if !(*pl).next.is_null() {
          (*pl).width += 1;
        }
      }
      self.len += 1;
      Cursor { node: x, slot }
    }
  }

  /// Removes the path's target entry, which must exist, and returns it.
  #[inline(always)]
  pub(crate) unsafe fn remove_at(&mut self, path: &mut Path<K, V>) -> (K, V) {
    unsafe {
      path.normalize();
      let head = self.head();
      let x = path.node();
      let slot = path.slot;
      let xlen = len(x);
      debug_assert!(x != head && slot < xlen);
      if xlen == 1 {
        let mut preds = [(head, 0usize); MAX_HEIGHT];
        self.preds_of(x, path.start(), &mut preds);
        let pair = read_entry(x, 0);
        set_len(x, 0);
        self.unlink_empty(x, &preds, 1);
        return pair;
      }
      let pair = read_entry(x, slot);
      shift_entries(x, slot + 1, slot, xlen - slot - 1);
      set_len(x, xlen - 1);
      for level in 0..self.height {
        let pl = link(path.pred(level).0, level);
        if !(*pl).next.is_null() {
          (*pl).width -= 1;
        }
      }
      self.len -= 1;
      pair
    }
  }

  /// Splices the empty node `x` out of every level. `preds` are the nodes before it. `removed`
  /// entries were taken from `x` without the links being told; the widths spanning it drop by that
  /// much.
  #[inline(always)]
  unsafe fn unlink_empty(&mut self, x: Ptr<K, V>, preds: &[(Ptr<K, V>, usize); MAX_HEIGHT], removed: usize) {
    unsafe {
      let xh = height(x);
      for (level, pred) in preds.iter().enumerate().take(self.height) {
        let pl = link(pred.0, level);
        if level < xh {
          let xl = *link(x, level);
          *pl = if xl.next.is_null() {
            Link::EMPTY
          } else {
            Link {
              next: xl.next,
              width: (*pl).width + xl.width - removed,
            }
          };
        } else if !(*pl).next.is_null() {
          (*pl).width -= removed;
        }
      }
      let after = next(x, 0);
      let prev = (*x).prev;
      if after.is_null() {
        self.tail = prev;
      } else {
        (*after).prev = prev;
      }
      free_node(self.head(), x);
      self.len -= removed;
      self.shrink();
    }
  }

  #[inline(always)]
  fn shrink(&mut self) {
    unsafe {
      while self.height > 0 && next(self.head(), self.height - 1).is_null() {
        self.height -= 1;
      }
    }
  }

  pub(crate) fn pop_first(&mut self) -> Option<(K, V)> {
    if self.len == 0 {
      return None;
    }
    unsafe {
      let mut path = self.path();
      self.path_to(0, &mut path);
      Some(self.remove_at(&mut path))
    }
  }

  pub(crate) fn pop_last(&mut self) -> Option<(K, V)> {
    if self.len == 0 {
      return None;
    }
    unsafe {
      let mut path = self.path();
      self.path_to(self.len - 1, &mut path);
      Some(self.remove_at(&mut path))
    }
  }

  pub(crate) fn push_front(&mut self, key: K, value: V) -> Cursor<K, V> {
    unsafe {
      let mut path = self.path();
      self.path_to(0, &mut path);
      self.insert_at(&mut path, key, value)
    }
  }

  pub(crate) fn push_back(&mut self, key: K, value: V) -> Cursor<K, V> {
    unsafe {
      let mut path = self.path();
      self.path_to(self.len, &mut path);
      self.insert_at(&mut path, key, value)
    }
  }

  /// Moves every entry from the path's target on into a new list that shares this list's chunk
  /// family.
  pub(crate) unsafe fn split_off(&mut self, path: &mut Path<K, V>) -> Self {
    unsafe {
      path.normalize();
      let family = slab_of(self.head()).family().clone();
      let mut rest = RawList::with(family, Levels::seeded(self.levels.fork()));
      let kept = path.index();
      if kept == self.len {
        return rest;
      }
      let head = self.head();
      let x = path.node();
      let slot = path.slot;
      let mut preds = [(head, 0usize); MAX_HEIGHT];
      if slot == 0 {
        self.preds_of(x, path.start(), &mut preds);
      } else {
        let xh = height(x);
        let start_x = path.start();
        self.split_node(x, slot, path);
        for (level, pred) in preds.iter_mut().enumerate().take(self.height) {
          *pred = if level < xh { (x, start_x) } else { path.pred(level) };
        }
      }
      let rest_head = rest.head();
      for (level, &(pred, pstart)) in preds.iter().enumerate().take(self.height) {
        let pl = link(pred, level);
        let l = *pl;
        if !l.next.is_null() {
          *link(rest_head, level) = Link {
            next: l.next,
            width: pstart + l.width - kept,
          };
          *pl = Link::EMPTY;
        }
      }
      (*next(rest_head, 0)).prev = ptr::null();
      rest.tail = self.tail;
      rest.len = self.len - kept;
      rest.height = self.height;
      rest.shrink();
      self.tail = if preds[0].0 == head { ptr::null() } else { preds[0].0 };
      self.len = kept;
      self.shrink();
      rest
    }
  }

  /// Moves every entry of `other` after the last entry of `self`, leaving `other` empty.
  pub(crate) fn append(&mut self, other: &mut Self) {
    if other.len == 0 {
      return;
    }
    if self.len == 0 {
      mem::swap(self, other);
      return;
    }
    unsafe {
      let mut path = self.path();
      self.path_to(self.len, &mut path);
      let n = self.len;
      let other_head = other.head();
      for level in 0..other.height {
        let ol = *link(other_head, level);
        if !ol.next.is_null() {
          let (pred, pstart) = path.pred(level);
          *link(pred, level) = Link {
            next: ol.next,
            width: n - pstart + ol.width,
          };
          *link(other_head, level) = Link::EMPTY;
        }
      }
      (*next(self.tail.cast_mut(), 0)).prev = self.tail;
      self.tail = other.tail;
      self.len += other.len;
      if other.height > self.height {
        self.height = other.height;
      }
      other.tail = ptr::null();
      other.len = 0;
      other.height = 0;
      slab_of(self.head()).absorb(slab_of(other_head));
    }
  }

  /// Keeps only the entries for which `f` returns `true`, visiting them in order. The list stays
  /// consistent if `f` panics.
  pub(crate) fn retain(&mut self, mut f: impl FnMut(&K, &mut V) -> bool) {
    unsafe {
      let head = self.head();
      let mut last = [(head, 0usize); MAX_HEIGHT];
      let mut start = 0usize;
      let mut cur = next(head, 0);
      while !cur.is_null() {
        let after = next(cur, 0);
        let xh = height(cur);
        let kept;
        {
          let _guard = RetainGuard {
            list: self as *mut Self,
            node: cur,
            preds: last,
          };
          let mut i = 0;
          while i < len(cur) {
            if f(&*keys(cur).add(i), &mut *vals(cur).add(i)) {
              i += 1;
              continue;
            }
            drop_entry(cur, i);
            let n = len(cur);
            shift_entries(cur, i + 1, i, n - i - 1);
            set_len(cur, n - 1);
            for (level, last) in last.iter().enumerate().take(self.height) {
              let pred = if level < xh { cur } else { last.0 };
              let pl = link(pred, level);
              if !(*pl).next.is_null() {
                (*pl).width -= 1;
              }
            }
            self.len -= 1;
          }
          kept = len(cur);
        }
        if kept > 0 {
          for pred in last.iter_mut().take(xh) {
            *pred = (cur, start);
          }
          start += kept;
        }
        cur = after;
      }
    }
  }

  pub(crate) fn clear(&mut self) {
    unsafe {
      let head = self.head();
      let mut cur = next(head, 0);
      for level in 0..self.height {
        *link(head, level) = Link::EMPTY;
      }
      self.tail = ptr::null();
      self.len = 0;
      self.height = 0;
      while !cur.is_null() {
        let after = next(cur, 0);
        drop_entries(cur);
        cur = after;
      }
      slab_of(head).reset();
    }
  }

  /// Builds a list holding `pairs` in the order given, with full nodes.
  pub(crate) fn from_ordered(pairs: impl Iterator<Item = (K, V)>) -> Self {
    let mut list = RawList::new();
    unsafe {
      let head = list.head();
      let mut tails = [(head, 0usize); MAX_HEIGHT];
      let mut cur: Ptr<K, V> = ptr::null_mut();
      for (key, value) in pairs {
        if cur.is_null() || len(cur) == cap::<K, V>() {
          let h = list.levels.next();
          if h > list.height {
            list.height = h;
          }
          let node = alloc_node(head, h, cur);
          let start = list.len;
          for (level, tail) in tails.iter_mut().enumerate().take(h) {
            *link(node, level) = Link::EMPTY;
            *link(tail.0, level) = Link {
              next: node,
              width: start - tail.1,
            };
            *tail = (node, start);
          }
          list.tail = node;
          cur = node;
        }
        let n = len(cur);
        write_entry(cur, n, key, value);
        set_len(cur, n + 1);
        list.len += 1;
      }
    }
    list
  }

  #[inline(always)]
  pub(crate) fn nodes(&self) -> Nodes<K, V> {
    Nodes {
      front: self.first(),
      back: self.last(),
      len: self.len,
    }
  }

  pub(crate) fn into_nodes(self) -> IntoNodes<K, V> {
    let list = ManuallyDrop::new(self);
    IntoNodes {
      head: list.head,
      nodes: list.nodes(),
    }
  }

  #[cfg(test)]
  pub(crate) fn check_invariants(&self, in_order: impl Fn(&K, &K) -> bool) {
    unsafe {
      let head = self.head();
      let mut nodes: Vec<(Ptr<K, V>, usize)> = Vec::new();
      let mut cur = next(head, 0);
      let mut prev: Ptr<K, V> = ptr::null_mut();
      let mut start = 0usize;
      let mut last_key: Option<*const K> = None;
      while !cur.is_null() {
        assert_eq!((*cur).prev, prev as *const _, "prev link");
        let h = height(cur);
        assert!(h >= 1 && h <= self.height, "node height {h} vs {}", self.height);
        let n = len(cur);
        assert!(n >= 1 && n <= cap::<K, V>(), "node len {n}");
        let ks = key_slice(cur);
        for w in ks.windows(2) {
          assert!(in_order(&w[0], &w[1]), "keys out of order inside a node");
        }
        if let Some(lk) = last_key {
          assert!(in_order(&*lk, &ks[0]), "keys out of order across nodes");
        }
        last_key = Some(&ks[n - 1]);
        nodes.push((cur, start));
        start += n;
        prev = cur;
        cur = next(cur, 0);
      }
      assert_eq!(start, self.len, "len");
      assert_eq!(self.tail, prev as *const _, "tail");
      for level in 0..MAX_HEIGHT {
        let expected: Vec<(Ptr<K, V>, usize)> = nodes.iter().copied().filter(|(n, _)| height(*n) > level).collect();
        let mut actual = Vec::new();
        let mut x = head;
        let mut pos = 0;
        loop {
          let l = *link(x, level);
          if l.next.is_null() {
            break;
          }
          pos += l.width;
          actual.push((l.next, pos));
          x = l.next;
        }
        assert_eq!(actual, expected, "level {level}");
        if level >= self.height {
          assert!(actual.is_empty(), "level {level} above height {}", self.height);
        }
      }
    }
  }
}

/// Unlinks the node under `retain` if it is empty when the guard drops, which happens both on the
/// normal path and when the predicate panics.
struct RetainGuard<K, V> {
  list: *mut RawList<K, V>,
  node: Ptr<K, V>,
  preds: [(Ptr<K, V>, usize); MAX_HEIGHT],
}

impl<K, V> Drop for RetainGuard<K, V> {
  fn drop(&mut self) {
    unsafe {
      if len(self.node) == 0 {
        (*self.list).unlink_empty(self.node, &self.preds, 0);
      }
    }
  }
}

impl<K, V> Drop for RawList<K, V> {
  fn drop(&mut self) {
    let _head = FreeHead(self.head());
    if mem::needs_drop::<K>() || mem::needs_drop::<V>() {
      unsafe {
        let mut cur = next(self.head(), 0);
        while !cur.is_null() {
          let after = next(cur, 0);
          drop_entries(cur);
          cur = after;
        }
      }
    }
  }
}

struct FreeHead<K, V>(Ptr<K, V>);

impl<K, V> Drop for FreeHead<K, V> {
  fn drop(&mut self) {
    unsafe { free_head(self.0) }
  }
}

/// A run of `len` consecutive entries from `front` to `back`.
pub(crate) struct Nodes<K, V> {
  pub(crate) front: Cursor<K, V>,
  pub(crate) back: Cursor<K, V>,
  pub(crate) len: usize,
}

impl<K, V> Clone for Nodes<K, V> {
  fn clone(&self) -> Self {
    *self
  }
}

impl<K, V> Copy for Nodes<K, V> {}

impl<K, V> Nodes<K, V> {
  pub(crate) const EMPTY: Self = Nodes {
    front: Cursor::NONE,
    back: Cursor::NONE,
    len: 0,
  };

  #[inline(always)]
  pub(crate) fn next(&mut self) -> Option<Cursor<K, V>> {
    if self.len == 0 {
      return None;
    }
    self.len -= 1;
    let c = self.front;
    if self.len > 0 {
      unsafe {
        let slot = c.slot + 1;
        if slot < len(c.node.cast_mut()) {
          self.front.slot = slot;
        } else {
          self.front = Cursor {
            node: next(c.node.cast_mut(), 0),
            slot: 0,
          };
        }
      }
    }
    Some(c)
  }

  #[inline(always)]
  pub(crate) fn next_back(&mut self) -> Option<Cursor<K, V>> {
    if self.len == 0 {
      return None;
    }
    self.len -= 1;
    let c = self.back;
    if self.len > 0 {
      unsafe {
        if c.slot > 0 {
          self.back.slot = c.slot - 1;
        } else {
          let p = (*c.node).prev;
          self.back = Cursor {
            node: p,
            slot: len(p.cast_mut()) - 1,
          };
        }
      }
    }
    Some(c)
  }
}

/// Owning run over every entry of a list, freeing the list's memory when dropped.
pub(crate) struct IntoNodes<K, V> {
  head: NonNull<Node<K, V>>,
  nodes: Nodes<K, V>,
}

unsafe impl<K: Send, V: Send> Send for IntoNodes<K, V> {}
unsafe impl<K: Sync, V: Sync> Sync for IntoNodes<K, V> {}

impl<K, V> IntoNodes<K, V> {
  #[inline(always)]
  pub(crate) fn len(&self) -> usize {
    self.nodes.len
  }

  #[inline(always)]
  pub(crate) fn next(&mut self) -> Option<(K, V)> {
    self
      .nodes
      .next()
      .map(|c| unsafe { read_entry(c.node.cast_mut(), c.slot) })
  }

  #[inline(always)]
  pub(crate) fn next_back(&mut self) -> Option<(K, V)> {
    self
      .nodes
      .next_back()
      .map(|c| unsafe { read_entry(c.node.cast_mut(), c.slot) })
  }
}

impl<K, V> Drop for IntoNodes<K, V> {
  fn drop(&mut self) {
    let _head = FreeHead(self.head.as_ptr());
    while let Some(c) = self.nodes.next() {
      unsafe { drop_entry(c.node.cast_mut(), c.slot) }
    }
  }
}
