use crate::node::{Node, next};
use crate::raw::{IntoNodes, Nodes, RawList};
use std::borrow::Borrow;
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::iter::FusedIterator;
use std::marker::PhantomData;
use std::mem;
use std::ops::{Bound, RangeBounds};

#[cfg(test)]
mod tests;

/// A sorted list that keeps equal elements, in insertion order, with O(log n) access by index.
pub struct OrderedSkipList<T> {
  raw: RawList<T, ()>,
}

impl<T> OrderedSkipList<T> {
  /// Makes a new, empty list.
  pub fn new() -> Self {
    OrderedSkipList { raw: RawList::new() }
  }

  /// Returns the number of elements.
  #[inline]
  pub fn len(&self) -> usize {
    self.raw.len()
  }

  /// Returns `true` if the list contains no elements.
  #[inline]
  pub fn is_empty(&self) -> bool {
    self.raw.len() == 0
  }

  /// Removes every element.
  pub fn clear(&mut self) {
    self.raw.clear()
  }

  /// Returns the smallest element.
  pub fn first(&self) -> Option<&T> {
    unsafe { self.raw.first().as_ref().map(|n| &n.key) }
  }

  /// Returns the largest element.
  pub fn last(&self) -> Option<&T> {
    unsafe { self.raw.last().as_ref().map(|n| &n.key) }
  }

  /// Returns the element at `index`.
  pub fn get_by_index(&self, index: usize) -> Option<&T> {
    if index >= self.len() {
      return None;
    }
    unsafe { Some(&(*self.raw.node_at(index + 1)).key) }
  }

  /// Inserts `value` after every element equal to it.
  pub fn insert(&mut self, value: T)
  where
    T: Ord,
  {
    unsafe {
      let mut path = self.raw.path();
      self.raw.path_where(|k| k <= &value, &mut path);
      self.raw.insert_at(&path, value, ());
    }
  }

  /// Returns `true` if an element equal to `value` is present.
  pub fn contains<Q>(&self, value: &Q) -> bool
  where
    T: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    self.get(value).is_some()
  }

  /// Returns the first element equal to `value`.
  pub fn get<Q>(&self, value: &Q) -> Option<&T>
  where
    T: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    unsafe {
      let (x, _) = self.raw.last_where(|k| k.borrow() < value);
      let n = next(x, 0);
      if n.is_null() || (*n).key.borrow() != value {
        return None;
      }
      Some(&(*n).key)
    }
  }

  /// Returns the index of the first element equal to `value`.
  pub fn rank<Q>(&self, value: &Q) -> Option<usize>
  where
    T: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    unsafe {
      let (x, pos) = self.raw.last_where(|k| k.borrow() < value);
      let n = next(x, 0);
      if n.is_null() || (*n).key.borrow() != value {
        return None;
      }
      Some(pos)
    }
  }

  /// Returns how many elements equal `value`.
  pub fn count<Q>(&self, value: &Q) -> usize
  where
    T: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    unsafe {
      let (_, lower) = self.raw.last_where(|k| k.borrow() < value);
      let (_, upper) = self.raw.last_where(|k| k.borrow() <= value);
      upper - lower
    }
  }

  /// Removes and returns the first element equal to `value`.
  pub fn take<Q>(&mut self, value: &Q) -> Option<T>
  where
    T: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    unsafe {
      let mut path = self.raw.path();
      self.raw.path_where(|k| k.borrow() < value, &mut path);
      let n = next(path.node(), 0);
      if n.is_null() || (*n).key.borrow() != value {
        return None;
      }
      Some(self.raw.unlink(n, &path).0)
    }
  }

  /// Removes and returns the last element equal to `value`.
  pub fn take_last<Q>(&mut self, value: &Q) -> Option<T>
  where
    T: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    unsafe {
      let (n, pos) = self.raw.last_where(|k| k.borrow() <= value);
      if pos == 0 || (*n).key.borrow() != value {
        return None;
      }
      let mut path = self.raw.path();
      self.raw.path_to(pos, &mut path);
      Some(self.raw.unlink(n, &path).0)
    }
  }

  /// Removes and returns the element at `index`.
  ///
  /// # Panics
  ///
  /// Panics if `index >= len`.
  pub fn remove(&mut self, index: usize) -> T {
    let len = self.len();
    assert!(index < len, "removal index (is {index}) should be < len (is {len})");
    unsafe {
      let mut path = self.raw.path();
      self.raw.path_to(index + 1, &mut path);
      let node = next(path.node(), 0);
      self.raw.unlink(node, &path).0
    }
  }

  /// Removes and returns the smallest element.
  pub fn pop_first(&mut self) -> Option<T> {
    self.raw.pop_first().map(|(v, _)| v)
  }

  /// Removes and returns the largest element.
  pub fn pop_last(&mut self) -> Option<T> {
    self.raw.pop_last().map(|(v, _)| v)
  }

  /// Splits the list in two at `value`: `self` keeps the elements below it and the returned list
  /// gets the rest.
  pub fn split_off<Q>(&mut self, value: &Q) -> Self
  where
    T: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    unsafe {
      let mut path = self.raw.path();
      self.raw.path_where(|k| k.borrow() < value, &mut path);
      OrderedSkipList {
        raw: self.raw.split_off(&path),
      }
    }
  }

  /// Splits the list in two at `at`: `self` keeps the first `at` elements and the returned list
  /// gets the rest.
  ///
  /// # Panics
  ///
  /// Panics if `at > len`.
  pub fn split_off_index(&mut self, at: usize) -> Self {
    let len = self.len();
    assert!(at <= len, "split index (is {at}) should be <= len (is {len})");
    unsafe {
      let mut path = self.raw.path();
      self.raw.path_to(at + 1, &mut path);
      OrderedSkipList {
        raw: self.raw.split_off(&path),
      }
    }
  }

  /// Moves every element of `other` into `self`, leaving `other` empty. Elements of `self` stay
  /// ahead of equal elements from `other`.
  pub fn append(&mut self, other: &mut Self)
  where
    T: Ord,
  {
    if other.is_empty() {
      return;
    }
    if self.is_empty() {
      mem::swap(self, other);
      return;
    }
    unsafe {
      if (*self.raw.last()).key <= (*other.raw.first()).key {
        self.raw.append(&mut other.raw);
        return;
      }
    }
    let a = mem::replace(&mut self.raw, RawList::new()).into_nodes();
    let b = mem::replace(&mut other.raw, RawList::new()).into_nodes();
    self.raw = RawList::from_ordered(merge_stable(a, b));
  }

  /// Keeps only the elements for which `f` returns `true`, visiting them in order.
  pub fn retain<F>(&mut self, mut f: F)
  where
    F: FnMut(&T) -> bool,
  {
    self.raw.retain(|v, _| f(v))
  }

  /// Iterates over the elements in order.
  pub fn iter(&self) -> Iter<'_, T> {
    Iter {
      nodes: self.raw.nodes(),
      marker: PhantomData,
    }
  }

  /// Iterates over the elements that fall in `range`.
  ///
  /// # Panics
  ///
  /// Panics if the start is greater than the end. Panics if both are equal and excluded.
  pub fn range<Q, R>(&self, range: R) -> Range<'_, T>
  where
    Q: Ord + ?Sized,
    T: Borrow<Q>,
    R: RangeBounds<Q>,
  {
    match (range.start_bound(), range.end_bound()) {
      (Bound::Excluded(s), Bound::Excluded(e)) if s == e => {
        panic!("range start and end are equal and excluded in OrderedSkipList")
      }
      (Bound::Included(s) | Bound::Excluded(s), Bound::Included(e) | Bound::Excluded(e)) if s > e => {
        panic!("range start is greater than range end in OrderedSkipList")
      }
      _ => {}
    }
    let nodes = unsafe {
      let (front, fpos) = match range.start_bound() {
        Bound::Unbounded => (self.raw.first(), 1),
        Bound::Included(s) => {
          let (x, p) = self.raw.last_where(|k| k.borrow() < s);
          (next(x, 0), p + 1)
        }
        Bound::Excluded(s) => {
          let (x, p) = self.raw.last_where(|k| k.borrow() <= s);
          (next(x, 0), p + 1)
        }
      };
      if front.is_null() {
        Nodes::EMPTY
      } else {
        let (back, bpos) = match range.end_bound() {
          Bound::Unbounded => (self.raw.last(), self.raw.len()),
          Bound::Included(e) => self.raw.last_where(|k| k.borrow() <= e),
          Bound::Excluded(e) => self.raw.last_where(|k| k.borrow() < e),
        };
        if bpos < fpos {
          Nodes::EMPTY
        } else {
          Nodes {
            front,
            back,
            len: bpos - fpos + 1,
          }
        }
      }
    };
    Range(Iter {
      nodes,
      marker: PhantomData,
    })
  }

  #[cfg(test)]
  pub(crate) fn check_invariants(&self)
  where
    T: Ord,
  {
    self.raw.check_invariants(|a, b| a <= b)
  }
}

/// Merges two sorted runs, keeping every element. On equal elements `a` goes first.
fn merge_stable<T: Ord>(a: IntoNodes<T, ()>, b: IntoNodes<T, ()>) -> impl Iterator<Item = (T, ())> {
  let mut a = a;
  let mut b = b;
  let mut a = std::iter::from_fn(move || a.next()).peekable();
  let mut b = std::iter::from_fn(move || b.next()).peekable();
  std::iter::from_fn(move || match (a.peek(), b.peek()) {
    (None, None) => None,
    (Some(_), None) => a.next(),
    (None, Some(_)) => b.next(),
    (Some(x), Some(y)) => {
      if x.0 <= y.0 {
        a.next()
      } else {
        b.next()
      }
    }
  })
}

impl<T> Default for OrderedSkipList<T> {
  fn default() -> Self {
    OrderedSkipList::new()
  }
}

impl<T: Clone> Clone for OrderedSkipList<T> {
  fn clone(&self) -> Self {
    OrderedSkipList {
      raw: RawList::from_ordered(self.iter().map(|v| (v.clone(), ()))),
    }
  }
}

impl<T: Ord> FromIterator<T> for OrderedSkipList<T> {
  fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
    let mut values: Vec<T> = iter.into_iter().collect();
    values.sort();
    OrderedSkipList {
      raw: RawList::from_ordered(values.into_iter().map(|v| (v, ()))),
    }
  }
}

impl<T: Ord, const N: usize> From<[T; N]> for OrderedSkipList<T> {
  fn from(values: [T; N]) -> Self {
    values.into_iter().collect()
  }
}

impl<T: Ord> Extend<T> for OrderedSkipList<T> {
  fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
    for v in iter {
      self.insert(v);
    }
  }
}

impl<'a, T: Ord + Copy + 'a> Extend<&'a T> for OrderedSkipList<T> {
  fn extend<I: IntoIterator<Item = &'a T>>(&mut self, iter: I) {
    for v in iter {
      self.insert(*v);
    }
  }
}

impl<T: PartialEq> PartialEq for OrderedSkipList<T> {
  fn eq(&self, other: &Self) -> bool {
    self.len() == other.len() && self.iter().eq(other.iter())
  }
}

impl<T: Eq> Eq for OrderedSkipList<T> {}

impl<T: PartialOrd> PartialOrd for OrderedSkipList<T> {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    self.iter().partial_cmp(other.iter())
  }
}

impl<T: Ord> Ord for OrderedSkipList<T> {
  fn cmp(&self, other: &Self) -> Ordering {
    self.iter().cmp(other.iter())
  }
}

impl<T: Hash> Hash for OrderedSkipList<T> {
  fn hash<H: Hasher>(&self, state: &mut H) {
    state.write_usize(self.len());
    for v in self.iter() {
      v.hash(state);
    }
  }
}

impl<T: fmt::Debug> fmt::Debug for OrderedSkipList<T> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_list().entries(self.iter()).finish()
  }
}

impl<'a, T> IntoIterator for &'a OrderedSkipList<T> {
  type Item = &'a T;
  type IntoIter = Iter<'a, T>;

  fn into_iter(self) -> Iter<'a, T> {
    self.iter()
  }
}

impl<T> IntoIterator for OrderedSkipList<T> {
  type Item = T;
  type IntoIter = IntoIter<T>;

  fn into_iter(self) -> IntoIter<T> {
    IntoIter(self.raw.into_nodes())
  }
}

/// Iterator over the elements of an [`OrderedSkipList`] in order.
pub struct Iter<'a, T> {
  nodes: Nodes<T, ()>,
  marker: PhantomData<&'a Node<T, ()>>,
}

unsafe impl<T: Sync> Send for Iter<'_, T> {}
unsafe impl<T: Sync> Sync for Iter<'_, T> {}

impl<T> Clone for Iter<'_, T> {
  fn clone(&self) -> Self {
    Iter {
      nodes: self.nodes,
      marker: PhantomData,
    }
  }
}

impl<'a, T> Iterator for Iter<'a, T> {
  type Item = &'a T;

  #[inline]
  fn next(&mut self) -> Option<&'a T> {
    self.nodes.next().map(|n| unsafe { &(*n).key })
  }

  #[inline]
  fn size_hint(&self) -> (usize, Option<usize>) {
    (self.nodes.len, Some(self.nodes.len))
  }
}

impl<T> DoubleEndedIterator for Iter<'_, T> {
  #[inline]
  fn next_back(&mut self) -> Option<Self::Item> {
    self.nodes.next_back().map(|n| unsafe { &(*n).key })
  }
}

impl<T> ExactSizeIterator for Iter<'_, T> {}
impl<T> FusedIterator for Iter<'_, T> {}

/// Iterator over a sub-range of an [`OrderedSkipList`].
pub struct Range<'a, T>(Iter<'a, T>);

impl<T> Clone for Range<'_, T> {
  fn clone(&self) -> Self {
    Range(self.0.clone())
  }
}

impl<'a, T> Iterator for Range<'a, T> {
  type Item = &'a T;

  #[inline]
  fn next(&mut self) -> Option<&'a T> {
    self.0.next()
  }

  #[inline]
  fn size_hint(&self) -> (usize, Option<usize>) {
    self.0.size_hint()
  }
}

impl<T> DoubleEndedIterator for Range<'_, T> {
  #[inline]
  fn next_back(&mut self) -> Option<Self::Item> {
    self.0.next_back()
  }
}

impl<T> ExactSizeIterator for Range<'_, T> {}
impl<T> FusedIterator for Range<'_, T> {}

/// Owning iterator over the elements of an [`OrderedSkipList`] in order.
pub struct IntoIter<T>(IntoNodes<T, ()>);

impl<T> Iterator for IntoIter<T> {
  type Item = T;

  #[inline]
  fn next(&mut self) -> Option<T> {
    self.0.next().map(|(v, _)| v)
  }

  #[inline]
  fn size_hint(&self) -> (usize, Option<usize>) {
    (self.0.len(), Some(self.0.len()))
  }
}

impl<T> DoubleEndedIterator for IntoIter<T> {
  #[inline]
  fn next_back(&mut self) -> Option<T> {
    self.0.next_back().map(|(v, _)| v)
  }
}

impl<T> ExactSizeIterator for IntoIter<T> {}
impl<T> FusedIterator for IntoIter<T> {}
