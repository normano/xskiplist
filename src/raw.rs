use crate::node::{
  Levels, Link, Node, alloc_head, alloc_node, drop_contents, drop_node, free_head, init_node, link, next, read_node,
  slab_of, take_node,
};
use crate::slab::{Family, MAX_HEIGHT};
use std::cmp::Ordering;
use std::marker::PhantomData;
use std::mem::{self, ManuallyDrop, MaybeUninit};
use std::ptr::{self, NonNull};
use std::sync::Arc;

pub(crate) type Ptr<K, V> = *mut Node<K, V>;

/// Positions count level-0 steps from the head: the head is 0 and the first element is 1.
///
/// Level `l` below `height` holds the predecessor on that level and its position. Levels at or
/// above `height` are the head at position 0 and are never written.
pub(crate) struct Path<K, V> {
  head: Ptr<K, V>,
  height: usize,
  node: [MaybeUninit<Ptr<K, V>>; MAX_HEIGHT],
  pos: [MaybeUninit<usize>; MAX_HEIGHT],
}

impl<K, V> Path<K, V> {
  #[inline(always)]
  fn new(head: Ptr<K, V>, height: usize) -> Self {
    Path {
      head,
      height,
      node: [MaybeUninit::uninit(); MAX_HEIGHT],
      pos: [MaybeUninit::uninit(); MAX_HEIGHT],
    }
  }

  #[inline(always)]
  unsafe fn set(&mut self, level: usize, node: Ptr<K, V>, pos: usize) {
    unsafe {
      self.node.get_unchecked_mut(level).write(node);
      self.pos.get_unchecked_mut(level).write(pos);
    }
  }

  /// The predecessor on `level` and its position.
  #[inline(always)]
  pub(crate) fn pred(&self, level: usize) -> (Ptr<K, V>, usize) {
    if level < self.height {
      unsafe {
        (
          self.node.get_unchecked(level).assume_init(),
          self.pos.get_unchecked(level).assume_init(),
        )
      }
    } else {
      (self.head, 0)
    }
  }

  /// The level-0 predecessor: the node the path points just after.
  #[inline(always)]
  pub(crate) fn node(&self) -> Ptr<K, V> {
    self.pred(0).0
  }

  /// The position of the level-0 predecessor.
  #[inline(always)]
  pub(crate) fn pos(&self) -> usize {
    self.pred(0).1
  }
}

/// A skiplist with link widths, shared by every collection in the crate. Ordering is the caller's
/// business: every operation here is by position or by a monotone predicate over keys.
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
  pub(crate) fn height(&self) -> usize {
    self.height
  }

  #[inline(always)]
  pub(crate) fn head(&self) -> Ptr<K, V> {
    self.head.as_ptr()
  }

  #[inline(always)]
  pub(crate) fn first(&self) -> Ptr<K, V> {
    unsafe { next(self.head(), 0) }
  }

  #[inline(always)]
  pub(crate) fn last(&self) -> Ptr<K, V> {
    self.tail as Ptr<K, V>
  }

  /// Walks down moving right while `go_right(next.key)` holds and records where each level stops.
  #[inline(always)]
  pub(crate) unsafe fn path_where(&self, mut go_right: impl FnMut(&K) -> bool) -> Path<K, V> {
    unsafe {
      let mut path = Path::new(self.head(), self.height);
      let mut x = self.head();
      let mut pos = 0;
      let mut bound: Ptr<K, V> = ptr::null_mut();
      let mut level = self.height;
      while level > 0 {
        level -= 1;
        loop {
          let l = *link(x, level);
          if l.next == bound {
            break;
          }
          if go_right(&(*l.next).key) {
            pos += l.width;
            x = l.next;
          } else {
            bound = l.next;
            break;
          }
        }
        path.set(level, x, pos);
      }
      path
    }
  }

  /// Like [`path_where`](Self::path_where) with `cmp(key) == Less` as the go-right test, except
  /// that a key comparing `Equal` ends the walk at once with that node.
  #[inline(always)]
  pub(crate) unsafe fn search(&self, mut cmp: impl FnMut(&K) -> Ordering) -> Result<Path<K, V>, Ptr<K, V>> {
    unsafe {
      let mut path = Path::new(self.head(), self.height);
      let mut x = self.head();
      let mut pos = 0;
      let mut bound: Ptr<K, V> = ptr::null_mut();
      let mut level = self.height;
      while level > 0 {
        level -= 1;
        loop {
          let l = *link(x, level);
          if l.next == bound {
            break;
          }
          match cmp(&(*l.next).key) {
            Ordering::Less => {
              pos += l.width;
              x = l.next;
            }
            Ordering::Equal => return Err(l.next),
            Ordering::Greater => {
              bound = l.next;
              break;
            }
          }
        }
        path.set(level, x, pos);
      }
      Ok(path)
    }
  }

  /// The last node for which `go_right(key)` holds (the head if none), with its position.
  #[inline(always)]
  pub(crate) unsafe fn last_where(&self, mut go_right: impl FnMut(&K) -> bool) -> (Ptr<K, V>, usize) {
    unsafe {
      let mut x = self.head();
      let mut pos = 0;
      let mut bound: Ptr<K, V> = ptr::null_mut();
      let mut level = self.height;
      while level > 0 {
        level -= 1;
        loop {
          let l = *link(x, level);
          if l.next == bound {
            break;
          }
          if go_right(&(*l.next).key) {
            pos += l.width;
            x = l.next;
          } else {
            bound = l.next;
            break;
          }
        }
      }
      (x, pos)
    }
  }

  /// Path whose every level stops at the last node with position below `target`.
  #[inline(always)]
  pub(crate) unsafe fn path_to(&self, target: usize) -> Path<K, V> {
    unsafe {
      let mut path = Path::new(self.head(), self.height);
      let mut x = self.head();
      let mut pos = 0;
      let mut level = self.height;
      while level > 0 {
        level -= 1;
        loop {
          let l = *link(x, level);
          if l.next.is_null() || pos + l.width >= target {
            break;
          }
          pos += l.width;
          x = l.next;
        }
        path.set(level, x, pos);
      }
      path
    }
  }

  /// The node at `target`, which must be in `1..=len`.
  #[inline(always)]
  pub(crate) unsafe fn node_at(&self, target: usize) -> Ptr<K, V> {
    unsafe {
      let mut x = self.head();
      let mut pos = 0;
      let mut level = self.height;
      while level > 0 {
        level -= 1;
        loop {
          let l = *link(x, level);
          if l.next.is_null() || pos + l.width > target {
            break;
          }
          pos += l.width;
          x = l.next;
          if pos == target {
            return x;
          }
        }
      }
      x
    }
  }

  /// Inserts a node right after `path.node()` and returns it.
  #[inline(always)]
  pub(crate) unsafe fn insert_at(&mut self, path: &Path<K, V>, key: K, value: V) -> Ptr<K, V> {
    unsafe {
      let head = self.head();
      let p = path.pos() + 1;
      let height = self.levels.next();
      if height > self.height {
        self.height = height;
      }
      let node = alloc_node(head, height);
      let pred0 = path.node();
      init_node(
        node,
        key,
        value,
        height,
        if pred0 == head { ptr::null() } else { pred0 },
      );
      for level in 0..height {
        let (pred, ppos) = path.pred(level);
        let pl = link(pred, level);
        let old = *pl;
        *link(node, level) = if old.next.is_null() {
          Link::EMPTY
        } else {
          Link {
            next: old.next,
            width: ppos + old.width + 1 - p,
          }
        };
        *pl = Link {
          next: node,
          width: p - ppos,
        };
      }
      for level in height..self.height {
        let pl = link(path.pred(level).0, level);
        if !(*pl).next.is_null() {
          (*pl).width += 1;
        }
      }
      let after = next(node, 0);
      if after.is_null() {
        self.tail = node;
      } else {
        (*after).prev = node;
      }
      self.len += 1;
      node
    }
  }

  /// Unlinks `node`, which must follow `path.node()`. Moves its contents out.
  #[inline(always)]
  pub(crate) unsafe fn unlink(&mut self, node: Ptr<K, V>, path: &Path<K, V>) -> (K, V) {
    unsafe {
      let height = (*node).height;
      for level in 0..height {
        let pl = link(path.pred(level).0, level);
        let nl = *link(node, level);
        *pl = if nl.next.is_null() {
          Link::EMPTY
        } else {
          Link {
            next: nl.next,
            width: (*pl).width + nl.width - 1,
          }
        };
      }
      for level in height..self.height {
        let pl = link(path.pred(level).0, level);
        if !(*pl).next.is_null() {
          (*pl).width -= 1;
        }
      }
      self.detach(node);
      take_node(self.head(), node)
    }
  }

  /// Fixes `prev`, `tail`, `len` and `height` after `node` was spliced out of every level.
  #[inline(always)]
  unsafe fn detach(&mut self, node: Ptr<K, V>) {
    unsafe {
      let after = next(node, 0);
      let prev = (*node).prev;
      if after.is_null() {
        self.tail = prev;
      } else {
        (*after).prev = prev;
      }
      self.len -= 1;
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
    let node = self.first();
    if node.is_null() {
      return None;
    }
    unsafe {
      let head = self.head();
      let height = (*node).height;
      for level in 0..height {
        *link(head, level) = *link(node, level);
      }
      for level in height..self.height {
        let hl = link(head, level);
        if !(*hl).next.is_null() {
          (*hl).width -= 1;
        }
      }
      self.detach(node);
      Some(take_node(head, node))
    }
  }

  pub(crate) fn pop_last(&mut self) -> Option<(K, V)> {
    let node = self.last();
    if node.is_null() {
      return None;
    }
    unsafe {
      let path = self.path_to(self.len);
      Some(self.unlink(node, &path))
    }
  }

  pub(crate) fn push_front(&mut self, key: K, value: V) -> Ptr<K, V> {
    unsafe {
      let path = Path::new(self.head(), 0);
      self.insert_at(&path, key, value)
    }
  }

  pub(crate) fn push_back(&mut self, key: K, value: V) -> Ptr<K, V> {
    unsafe {
      let path = self.path_to(self.len + 1);
      self.insert_at(&path, key, value)
    }
  }

  /// Moves every node after `path.node()` into a new list that shares this list's chunk family.
  pub(crate) unsafe fn split_off(&mut self, path: &Path<K, V>) -> Self {
    unsafe {
      let family = slab_of(self.head()).family().clone();
      let mut rest = RawList::with(family, Levels::seeded(self.levels.fork()));
      let kept = path.pos();
      if kept == self.len {
        return rest;
      }
      let rest_head = rest.head();
      for level in 0..self.height {
        let (pred, ppos) = path.pred(level);
        let pl = link(pred, level);
        let l = *pl;
        if !l.next.is_null() {
          *link(rest_head, level) = Link {
            next: l.next,
            width: ppos + l.width - kept,
          };
          *pl = Link::EMPTY;
        }
      }
      (*next(rest_head, 0)).prev = ptr::null();
      rest.tail = self.tail;
      rest.len = self.len - kept;
      rest.height = self.height;
      rest.shrink();
      let pred0 = path.node();
      self.tail = if pred0 == self.head() { ptr::null() } else { pred0 };
      self.len = kept;
      self.shrink();
      rest
    }
  }

  /// Moves every node of `other` after the last node of `self`, leaving `other` empty.
  pub(crate) fn append(&mut self, other: &mut Self) {
    if other.len == 0 {
      return;
    }
    if self.len == 0 {
      mem::swap(self, other);
      return;
    }
    unsafe {
      let path = self.path_to(self.len + 1);
      let n = self.len;
      let other_head = other.head();
      for level in 0..other.height {
        let ol = *link(other_head, level);
        if !ol.next.is_null() {
          let (pred, ppos) = path.pred(level);
          *link(pred, level) = Link {
            next: ol.next,
            width: n - ppos + ol.width,
          };
          *link(other_head, level) = Link::EMPTY;
        }
      }
      (*next(self.last(), 0)).prev = self.tail;
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

  /// Keeps only the nodes for which `f` returns `true`, visiting them in order. The list stays
  /// consistent if `f` panics.
  pub(crate) fn retain(&mut self, mut f: impl FnMut(&K, &mut V) -> bool) {
    unsafe {
      let head = self.head();
      let mut last = [head; MAX_HEIGHT];
      let mut cur = self.first();
      while !cur.is_null() {
        let after = next(cur, 0);
        let height = (*cur).height;
        if f(&(*cur).key, &mut (*cur).value) {
          last[..height].fill(cur);
        } else {
          for (level, pred) in last.iter().enumerate().take(height) {
            let pl = link(*pred, level);
            let nl = *link(cur, level);
            *pl = if nl.next.is_null() {
              Link::EMPTY
            } else {
              Link {
                next: nl.next,
                width: (*pl).width + nl.width - 1,
              }
            };
          }
          for (level, pred) in last.iter().enumerate().take(self.height).skip(height) {
            let pl = link(*pred, level);
            if !(*pl).next.is_null() {
              (*pl).width -= 1;
            }
          }
          self.detach(cur);
          drop_node(head, cur);
        }
        cur = after;
      }
    }
  }

  pub(crate) fn clear(&mut self) {
    unsafe {
      let head = self.head();
      let mut cur = self.first();
      for level in 0..self.height {
        *link(head, level) = Link::EMPTY;
      }
      self.tail = ptr::null();
      self.len = 0;
      self.height = 0;
      while !cur.is_null() {
        let after = next(cur, 0);
        drop_contents(cur);
        cur = after;
      }
      slab_of(head).reset();
    }
  }

  /// Builds a list holding `pairs` in the order given.
  pub(crate) fn from_ordered(pairs: impl Iterator<Item = (K, V)>) -> Self {
    let mut list = RawList::new();
    unsafe {
      let head = list.head();
      let mut tails = [head; MAX_HEIGHT];
      let mut tpos = [0usize; MAX_HEIGHT];
      for (key, value) in pairs {
        let height = list.levels.next();
        if height > list.height {
          list.height = height;
        }
        let p = list.len + 1;
        let node = alloc_node(head, height);
        init_node(
          node,
          key,
          value,
          height,
          if tails[0] == head { ptr::null() } else { tails[0] },
        );
        for (level, tail) in tails.iter_mut().enumerate().take(height) {
          *link(node, level) = Link::EMPTY;
          *link(*tail, level) = Link {
            next: node,
            width: p - tpos[level],
          };
          *tail = node;
          tpos[level] = p;
        }
        list.tail = node;
        list.len = p;
      }
    }
    list
  }

  #[inline(always)]
  pub(crate) fn nodes(&self) -> Nodes<K, V> {
    Nodes {
      front: self.first(),
      back: self.tail,
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
      let mut nodes = Vec::new();
      let mut cur = self.first();
      let mut prev: Ptr<K, V> = ptr::null_mut();
      while !cur.is_null() {
        assert_eq!((*cur).prev, prev as *const _, "prev link");
        let height = (*cur).height;
        assert!(
          height >= 1 && height <= self.height,
          "node height {height} vs {}",
          self.height
        );
        if !prev.is_null() {
          assert!(in_order(&(*prev).key, &(*cur).key), "keys out of order");
        }
        nodes.push(cur);
        prev = cur;
        cur = next(cur, 0);
      }
      assert_eq!(nodes.len(), self.len, "len");
      assert_eq!(self.tail, prev as *const _, "tail");
      for level in 0..MAX_HEIGHT {
        let expected: Vec<(Ptr<K, V>, usize)> = nodes
          .iter()
          .enumerate()
          .filter(|(_, n)| (***n).height > level)
          .map(|(i, n)| (*n, i + 1))
          .collect();
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

impl<K, V> Drop for RawList<K, V> {
  fn drop(&mut self) {
    let _head = FreeHead(self.head());
    if mem::needs_drop::<K>() || mem::needs_drop::<V>() {
      unsafe {
        let mut cur = self.first();
        while !cur.is_null() {
          let after = next(cur, 0);
          drop_contents(cur);
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

/// A run of `len` consecutive nodes from `front` to `back`.
pub(crate) struct Nodes<K, V> {
  pub(crate) front: *const Node<K, V>,
  pub(crate) back: *const Node<K, V>,
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
    front: ptr::null(),
    back: ptr::null(),
    len: 0,
  };

  #[inline(always)]
  pub(crate) fn next(&mut self) -> Option<Ptr<K, V>> {
    if self.len == 0 {
      return None;
    }
    self.len -= 1;
    let node = self.front as Ptr<K, V>;
    unsafe {
      self.front = next(node, 0);
    }
    Some(node)
  }

  #[inline(always)]
  pub(crate) fn next_back(&mut self) -> Option<Ptr<K, V>> {
    if self.len == 0 {
      return None;
    }
    self.len -= 1;
    let node = self.back as Ptr<K, V>;
    unsafe {
      self.back = (*node).prev;
    }
    Some(node)
  }
}

/// Owning run over every node of a list, freeing the list's memory when dropped.
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
    self.nodes.next().map(|node| unsafe { read_node(node) })
  }

  #[inline(always)]
  pub(crate) fn next_back(&mut self) -> Option<(K, V)> {
    self.nodes.next_back().map(|node| unsafe { read_node(node) })
  }
}

impl<K, V> Drop for IntoNodes<K, V> {
  fn drop(&mut self) {
    let _head = FreeHead(self.head.as_ptr());
    while let Some(node) = self.nodes.next() {
      unsafe { drop_contents(node) }
    }
  }
}
