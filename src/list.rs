use crate::raw::{IntoNodes, Nodes, RawList};
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::iter::FusedIterator;
use std::marker::PhantomData;
use std::ops::{Index, IndexMut};

#[cfg(test)]
mod tests;

/// A list with O(log n) access, insertion and removal at any index.
///
/// The API follows [`std::collections::VecDeque`] where the operations match.
pub struct SkipList<T> {
  raw: RawList<T, ()>,
}

impl<T> SkipList<T> {
  /// Makes a new, empty list.
  pub fn new() -> Self {
    SkipList { raw: RawList::new() }
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

  /// Returns the first element.
  pub fn front(&self) -> Option<&T> {
    unsafe { self.raw.first().pair().map(|(k, _)| k) }
  }

  /// Returns the first element mutably.
  pub fn front_mut(&mut self) -> Option<&mut T> {
    let c = self.raw.first();
    if c.is_null() {
      None
    } else {
      unsafe { Some(c.key_mut()) }
    }
  }

  /// Returns the last element.
  pub fn back(&self) -> Option<&T> {
    unsafe { self.raw.last().pair().map(|(k, _)| k) }
  }

  /// Returns the last element mutably.
  pub fn back_mut(&mut self) -> Option<&mut T> {
    let c = self.raw.last();
    if c.is_null() {
      None
    } else {
      unsafe { Some(c.key_mut()) }
    }
  }

  /// Returns the element at `index`.
  pub fn get(&self, index: usize) -> Option<&T> {
    if index >= self.len() {
      return None;
    }
    unsafe { Some(self.raw.node_at(index).key()) }
  }

  /// Returns the element at `index` mutably.
  pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
    if index >= self.len() {
      return None;
    }
    unsafe { Some(self.raw.node_at(index).key_mut()) }
  }

  /// Adds an element at the front.
  pub fn push_front(&mut self, value: T) {
    self.raw.push_front(value, ());
  }

  /// Adds an element at the back.
  pub fn push_back(&mut self, value: T) {
    self.raw.push_back(value, ());
  }

  /// Removes and returns the first element.
  pub fn pop_front(&mut self) -> Option<T> {
    self.raw.pop_first().map(|(v, _)| v)
  }

  /// Removes and returns the last element.
  pub fn pop_back(&mut self) -> Option<T> {
    self.raw.pop_last().map(|(v, _)| v)
  }

  /// Inserts `value` at `index`, shifting the elements after it.
  ///
  /// # Panics
  ///
  /// Panics if `index > len`.
  pub fn insert(&mut self, index: usize, value: T) {
    let len = self.len();
    assert!(index <= len, "insertion index (is {index}) should be <= len (is {len})");
    unsafe {
      let mut path = self.raw.path();
      self.raw.path_to(index, &mut path);
      self.raw.insert_at(&mut path, value, ());
    }
  }

  /// Removes and returns the element at `index`, shifting the elements after it.
  ///
  /// # Panics
  ///
  /// Panics if `index >= len`.
  pub fn remove(&mut self, index: usize) -> T {
    let len = self.len();
    assert!(index < len, "removal index (is {index}) should be < len (is {len})");
    unsafe {
      let mut path = self.raw.path();
      self.raw.path_to(index, &mut path);
      self.raw.remove_at(&mut path).0
    }
  }

  /// Splits the list in two at `at`: `self` keeps the first `at` elements and the returned list
  /// gets the rest.
  ///
  /// # Panics
  ///
  /// Panics if `at > len`.
  pub fn split_off(&mut self, at: usize) -> Self {
    let len = self.len();
    assert!(at <= len, "split index (is {at}) should be <= len (is {len})");
    unsafe {
      let mut path = self.raw.path();
      self.raw.path_to(at, &mut path);
      SkipList {
        raw: self.raw.split_off(&mut path),
      }
    }
  }

  /// Moves every element of `other` to the back of `self`, leaving `other` empty.
  pub fn append(&mut self, other: &mut Self) {
    self.raw.append(&mut other.raw)
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

  /// Iterates over the elements in order, mutably.
  pub fn iter_mut(&mut self) -> IterMut<'_, T> {
    IterMut {
      nodes: self.raw.nodes(),
      marker: PhantomData,
    }
  }

  #[cfg(test)]
  pub(crate) fn check_invariants(&self) {
    self.raw.check_invariants(|_, _| true)
  }
}

impl<T> Default for SkipList<T> {
  fn default() -> Self {
    SkipList::new()
  }
}

impl<T: Clone> Clone for SkipList<T> {
  fn clone(&self) -> Self {
    SkipList {
      raw: RawList::from_ordered(self.iter().map(|v| (v.clone(), ()))),
    }
  }
}

impl<T> FromIterator<T> for SkipList<T> {
  fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
    SkipList {
      raw: RawList::from_ordered(iter.into_iter().map(|v| (v, ()))),
    }
  }
}

impl<T, const N: usize> From<[T; N]> for SkipList<T> {
  fn from(values: [T; N]) -> Self {
    values.into_iter().collect()
  }
}

impl<T> Extend<T> for SkipList<T> {
  fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
    for v in iter {
      self.push_back(v);
    }
  }
}

impl<'a, T: Copy + 'a> Extend<&'a T> for SkipList<T> {
  fn extend<I: IntoIterator<Item = &'a T>>(&mut self, iter: I) {
    for v in iter {
      self.push_back(*v);
    }
  }
}

impl<T: PartialEq> PartialEq for SkipList<T> {
  fn eq(&self, other: &Self) -> bool {
    self.len() == other.len() && self.iter().eq(other.iter())
  }
}

impl<T: Eq> Eq for SkipList<T> {}

impl<T: PartialOrd> PartialOrd for SkipList<T> {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    self.iter().partial_cmp(other.iter())
  }
}

impl<T: Ord> Ord for SkipList<T> {
  fn cmp(&self, other: &Self) -> Ordering {
    self.iter().cmp(other.iter())
  }
}

impl<T: Hash> Hash for SkipList<T> {
  fn hash<H: Hasher>(&self, state: &mut H) {
    state.write_usize(self.len());
    for v in self.iter() {
      v.hash(state);
    }
  }
}

impl<T: fmt::Debug> fmt::Debug for SkipList<T> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_list().entries(self.iter()).finish()
  }
}

impl<T> Index<usize> for SkipList<T> {
  type Output = T;

  /// # Panics
  ///
  /// Panics if `index >= len`.
  fn index(&self, index: usize) -> &T {
    self.get(index).expect("index out of bounds")
  }
}

impl<T> IndexMut<usize> for SkipList<T> {
  fn index_mut(&mut self, index: usize) -> &mut T {
    self.get_mut(index).expect("index out of bounds")
  }
}

impl<'a, T> IntoIterator for &'a SkipList<T> {
  type Item = &'a T;
  type IntoIter = Iter<'a, T>;

  fn into_iter(self) -> Iter<'a, T> {
    self.iter()
  }
}

impl<'a, T> IntoIterator for &'a mut SkipList<T> {
  type Item = &'a mut T;
  type IntoIter = IterMut<'a, T>;

  fn into_iter(self) -> IterMut<'a, T> {
    self.iter_mut()
  }
}

impl<T> IntoIterator for SkipList<T> {
  type Item = T;
  type IntoIter = IntoIter<T>;

  fn into_iter(self) -> IntoIter<T> {
    IntoIter(self.raw.into_nodes())
  }
}

/// Iterator over the elements of a [`SkipList`] in order.
pub struct Iter<'a, T> {
  nodes: Nodes<T, ()>,
  marker: PhantomData<&'a T>,
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
    self.nodes.next().map(|c| unsafe { c.key() })
  }

  #[inline]
  fn size_hint(&self) -> (usize, Option<usize>) {
    (self.nodes.len, Some(self.nodes.len))
  }
}

impl<T> DoubleEndedIterator for Iter<'_, T> {
  #[inline]
  fn next_back(&mut self) -> Option<Self::Item> {
    self.nodes.next_back().map(|c| unsafe { c.key() })
  }
}

impl<T> ExactSizeIterator for Iter<'_, T> {}
impl<T> FusedIterator for Iter<'_, T> {}

/// Iterator over the elements of a [`SkipList`] in order, mutably.
pub struct IterMut<'a, T> {
  nodes: Nodes<T, ()>,
  marker: PhantomData<&'a mut T>,
}

unsafe impl<T: Send> Send for IterMut<'_, T> {}
unsafe impl<T: Sync> Sync for IterMut<'_, T> {}

impl<'a, T> Iterator for IterMut<'a, T> {
  type Item = &'a mut T;

  #[inline]
  fn next(&mut self) -> Option<&'a mut T> {
    self.nodes.next().map(|c| unsafe { c.key_mut() })
  }

  #[inline]
  fn size_hint(&self) -> (usize, Option<usize>) {
    (self.nodes.len, Some(self.nodes.len))
  }
}

impl<T> DoubleEndedIterator for IterMut<'_, T> {
  #[inline]
  fn next_back(&mut self) -> Option<Self::Item> {
    self.nodes.next_back().map(|c| unsafe { c.key_mut() })
  }
}

impl<T> ExactSizeIterator for IterMut<'_, T> {}
impl<T> FusedIterator for IterMut<'_, T> {}

/// Owning iterator over the elements of a [`SkipList`] in order.
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
