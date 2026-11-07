use crate::map::{self, SkipMap};
use std::borrow::Borrow;
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::iter::{FusedIterator, Peekable};
use std::ops::{BitAnd, BitOr, BitXor, RangeBounds, Sub};

#[cfg(test)]
mod tests;

/// An ordered set backed by a skiplist.
///
/// The API follows [`std::collections::BTreeSet`].
pub struct SkipSet<T> {
  map: SkipMap<T, ()>,
}

impl<T> SkipSet<T> {
  /// Makes a new, empty set.
  pub fn new() -> Self {
    SkipSet { map: SkipMap::new() }
  }

  /// Returns the number of elements in the set.
  #[inline]
  pub fn len(&self) -> usize {
    self.map.len()
  }

  /// Returns `true` if the set contains no elements.
  #[inline]
  pub fn is_empty(&self) -> bool {
    self.map.is_empty()
  }

  /// Removes every element.
  pub fn clear(&mut self) {
    self.map.clear()
  }

  /// Adds a value. Returns `false` and keeps the stored value if an equal one was present.
  pub fn insert(&mut self, value: T) -> bool
  where
    T: Ord,
  {
    self.map.insert(value, ()).is_none()
  }

  /// Adds a value, replacing the stored value equal to it. Returns the replaced value.
  pub fn replace(&mut self, value: T) -> Option<T>
  where
    T: Ord,
  {
    self.map.replace_key(value)
  }

  /// Returns `true` if the set contains `value`.
  pub fn contains<Q>(&self, value: &Q) -> bool
  where
    T: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    self.map.contains_key(value)
  }

  /// Returns the stored value equal to `value`.
  pub fn get<Q>(&self, value: &Q) -> Option<&T>
  where
    T: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    self.map.get_key_value(value).map(|(k, _)| k)
  }

  /// Removes `value`. Returns whether it was present.
  pub fn remove<Q>(&mut self, value: &Q) -> bool
  where
    T: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    self.map.remove_entry(value).is_some()
  }

  /// Removes and returns the stored value equal to `value`.
  pub fn take<Q>(&mut self, value: &Q) -> Option<T>
  where
    T: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    self.map.remove_entry(value).map(|(k, _)| k)
  }

  /// Gets the entry for `value` for in-place manipulation.
  pub fn entry(&mut self, value: T) -> Entry<'_, T>
  where
    T: Ord,
  {
    match self.map.entry(value) {
      map::Entry::Occupied(e) => Entry::Occupied(OccupiedEntry(e)),
      map::Entry::Vacant(e) => Entry::Vacant(VacantEntry(e)),
    }
  }

  /// Returns the smallest value.
  pub fn first(&self) -> Option<&T> {
    self.map.first_key_value().map(|(k, _)| k)
  }

  /// Returns the largest value.
  pub fn last(&self) -> Option<&T> {
    self.map.last_key_value().map(|(k, _)| k)
  }

  /// Removes and returns the smallest value.
  pub fn pop_first(&mut self) -> Option<T> {
    self.map.pop_first().map(|(k, _)| k)
  }

  /// Removes and returns the largest value.
  pub fn pop_last(&mut self) -> Option<T> {
    self.map.pop_last().map(|(k, _)| k)
  }

  /// Keeps only the values for which `f` returns `true`, visiting them in order.
  pub fn retain<F>(&mut self, mut f: F)
  where
    F: FnMut(&T) -> bool,
  {
    self.map.retain(|k, _| f(k))
  }

  /// Splits the set in two at `value`: `self` keeps the values below it and the returned set gets
  /// the rest.
  pub fn split_off<Q>(&mut self, value: &Q) -> Self
  where
    T: Borrow<Q> + Ord,
    Q: Ord + ?Sized,
  {
    SkipSet {
      map: self.map.split_off(value),
    }
  }

  /// Moves every value of `other` into `self`, leaving `other` empty.
  pub fn append(&mut self, other: &mut Self)
  where
    T: Ord,
  {
    self.map.append(&mut other.map)
  }

  /// Iterates over the values in order.
  pub fn iter(&self) -> Iter<'_, T> {
    Iter(self.map.keys())
  }

  /// Iterates over the values that fall in `range`.
  ///
  /// # Panics
  ///
  /// Panics if the start is greater than the end. Panics if both are equal and excluded.
  pub fn range<Q, R>(&self, range: R) -> Range<'_, T>
  where
    Q: Ord + ?Sized,
    T: Borrow<Q> + Ord,
    R: RangeBounds<Q>,
  {
    Range(self.map.range(range))
  }

  fn merge<'a>(&'a self, other: &'a Self, op: Op) -> Merge<'a, T> {
    Merge {
      a: self.iter().peekable(),
      b: other.iter().peekable(),
      op,
    }
  }

  /// Values in `self` or `other`, in order.
  pub fn union<'a>(&'a self, other: &'a Self) -> Union<'a, T> {
    Union(self.merge(other, Op::Union))
  }

  /// Values in both `self` and `other`, in order.
  pub fn intersection<'a>(&'a self, other: &'a Self) -> Intersection<'a, T> {
    Intersection(self.merge(other, Op::Intersection))
  }

  /// Values in `self` but not in `other`, in order.
  pub fn difference<'a>(&'a self, other: &'a Self) -> Difference<'a, T> {
    Difference(self.merge(other, Op::Difference))
  }

  /// Values in exactly one of `self` and `other`, in order.
  pub fn symmetric_difference<'a>(&'a self, other: &'a Self) -> SymmetricDifference<'a, T> {
    SymmetricDifference(self.merge(other, Op::SymmetricDifference))
  }

  #[cfg(test)]
  pub(crate) fn check_invariants(&self)
  where
    T: Ord,
  {
    self.map.check_invariants()
  }
}

/// A view into one value of a [`SkipSet`], which may be vacant or occupied.
#[allow(clippy::large_enum_variant)]
pub enum Entry<'a, T> {
  /// The value is not in the set.
  Vacant(VacantEntry<'a, T>),
  /// The value is in the set.
  Occupied(OccupiedEntry<'a, T>),
}

/// A view into a value that is not in the set.
pub struct VacantEntry<'a, T>(map::VacantEntry<'a, T, ()>);

/// A view into a value that is in the set.
pub struct OccupiedEntry<'a, T>(map::OccupiedEntry<'a, T, ()>);

impl<'a, T> VacantEntry<'a, T> {
  /// Returns the value that would be inserted.
  pub fn get(&self) -> &T {
    self.0.key()
  }

  /// Takes ownership of the value.
  pub fn into_value(self) -> T {
    self.0.into_key()
  }

  /// Inserts the value and returns a reference to it.
  pub fn insert(self) -> &'a T {
    self.0.insert_key_value(()).0
  }
}

impl<T: Ord> OccupiedEntry<'_, T> {
  /// Returns the stored value.
  pub fn get(&self) -> &T {
    self.0.key()
  }

  /// Removes and returns the stored value.
  pub fn remove(self) -> T {
    self.0.remove_entry().0
  }
}

impl<T> Default for SkipSet<T> {
  fn default() -> Self {
    SkipSet::new()
  }
}

impl<T: Clone> Clone for SkipSet<T> {
  fn clone(&self) -> Self {
    SkipSet { map: self.map.clone() }
  }
}

impl<T: Ord> FromIterator<T> for SkipSet<T> {
  fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
    SkipSet {
      map: iter.into_iter().map(|v| (v, ())).collect(),
    }
  }
}

impl<T: Ord, const N: usize> From<[T; N]> for SkipSet<T> {
  fn from(values: [T; N]) -> Self {
    values.into_iter().collect()
  }
}

impl<T: Ord> Extend<T> for SkipSet<T> {
  fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
    for v in iter {
      self.insert(v);
    }
  }
}

impl<'a, T: Ord + Copy + 'a> Extend<&'a T> for SkipSet<T> {
  fn extend<I: IntoIterator<Item = &'a T>>(&mut self, iter: I) {
    for v in iter {
      self.insert(*v);
    }
  }
}

impl<T: PartialEq> PartialEq for SkipSet<T> {
  fn eq(&self, other: &Self) -> bool {
    self.map == other.map
  }
}

impl<T: Eq> Eq for SkipSet<T> {}

impl<T: PartialOrd> PartialOrd for SkipSet<T> {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    self.iter().partial_cmp(other.iter())
  }
}

impl<T: Ord> Ord for SkipSet<T> {
  fn cmp(&self, other: &Self) -> Ordering {
    self.iter().cmp(other.iter())
  }
}

impl<T: Hash> Hash for SkipSet<T> {
  fn hash<H: Hasher>(&self, state: &mut H) {
    state.write_usize(self.len());
    for v in self.iter() {
      v.hash(state);
    }
  }
}

impl<T: fmt::Debug> fmt::Debug for SkipSet<T> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_set().entries(self.iter()).finish()
  }
}

impl<'a, T> IntoIterator for &'a SkipSet<T> {
  type Item = &'a T;
  type IntoIter = Iter<'a, T>;

  fn into_iter(self) -> Iter<'a, T> {
    self.iter()
  }
}

impl<T> IntoIterator for SkipSet<T> {
  type Item = T;
  type IntoIter = IntoIter<T>;

  fn into_iter(self) -> IntoIter<T> {
    IntoIter(self.map.into_keys())
  }
}

macro_rules! set_op {
  ($trait:ident, $method:ident, $op:ident) => {
    impl<T: Ord + Clone> $trait<&SkipSet<T>> for &SkipSet<T> {
      type Output = SkipSet<T>;

      fn $method(self, rhs: &SkipSet<T>) -> SkipSet<T> {
        SkipSet {
          map: SkipMap::from_sorted(self.merge(rhs, Op::$op).map(|v| (v.clone(), ()))),
        }
      }
    }
  };
}

set_op!(BitOr, bitor, Union);
set_op!(BitAnd, bitand, Intersection);
set_op!(Sub, sub, Difference);
set_op!(BitXor, bitxor, SymmetricDifference);

/// Iterator over the values of a [`SkipSet`] in order.
pub struct Iter<'a, T>(map::Keys<'a, T, ()>);

/// Iterator over a sub-range of a [`SkipSet`].
pub struct Range<'a, T>(map::Range<'a, T, ()>);

/// Owning iterator over the values of a [`SkipSet`] in order.
pub struct IntoIter<T>(map::IntoKeys<T, ()>);

impl<'a, T> Iterator for Iter<'a, T> {
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

impl<T> DoubleEndedIterator for Iter<'_, T> {
  #[inline]
  fn next_back(&mut self) -> Option<Self::Item> {
    self.0.next_back()
  }
}

impl<T> ExactSizeIterator for Iter<'_, T> {}
impl<T> FusedIterator for Iter<'_, T> {}

impl<'a, T> Iterator for Range<'a, T> {
  type Item = &'a T;

  #[inline]
  fn next(&mut self) -> Option<&'a T> {
    self.0.next().map(|(k, _)| k)
  }

  #[inline]
  fn size_hint(&self) -> (usize, Option<usize>) {
    self.0.size_hint()
  }
}

impl<T> DoubleEndedIterator for Range<'_, T> {
  #[inline]
  fn next_back(&mut self) -> Option<Self::Item> {
    self.0.next_back().map(|(k, _)| k)
  }
}

impl<T> ExactSizeIterator for Range<'_, T> {}
impl<T> FusedIterator for Range<'_, T> {}

impl<T> Iterator for IntoIter<T> {
  type Item = T;

  #[inline]
  fn next(&mut self) -> Option<T> {
    self.0.next()
  }

  #[inline]
  fn size_hint(&self) -> (usize, Option<usize>) {
    self.0.size_hint()
  }
}

impl<T> DoubleEndedIterator for IntoIter<T> {
  #[inline]
  fn next_back(&mut self) -> Option<T> {
    self.0.next_back()
  }
}

impl<T> ExactSizeIterator for IntoIter<T> {}
impl<T> FusedIterator for IntoIter<T> {}

#[derive(Clone, Copy)]
enum Op {
  Union,
  Intersection,
  Difference,
  SymmetricDifference,
}

struct Merge<'a, T> {
  a: Peekable<Iter<'a, T>>,
  b: Peekable<Iter<'a, T>>,
  op: Op,
}

impl<'a, T: Ord> Iterator for Merge<'a, T> {
  type Item = &'a T;

  fn next(&mut self) -> Option<&'a T> {
    loop {
      let ord = match (self.a.peek(), self.b.peek()) {
        (None, None) => return None,
        (Some(_), None) => {
          return match self.op {
            Op::Intersection => None,
            _ => self.a.next(),
          };
        }
        (None, Some(_)) => {
          return match self.op {
            Op::Intersection | Op::Difference => None,
            _ => self.b.next(),
          };
        }
        (Some(x), Some(y)) => x.cmp(y),
      };
      match (ord, self.op) {
        (Ordering::Less, Op::Intersection) => {
          self.a.next();
        }
        (Ordering::Less, _) => return self.a.next(),
        (Ordering::Greater, Op::Intersection | Op::Difference) => {
          self.b.next();
        }
        (Ordering::Greater, _) => return self.b.next(),
        (Ordering::Equal, Op::Union | Op::Intersection) => {
          self.b.next();
          return self.a.next();
        }
        (Ordering::Equal, Op::Difference | Op::SymmetricDifference) => {
          self.a.next();
          self.b.next();
        }
      }
    }
  }
}

macro_rules! set_iter {
  ($name:ident, $doc:literal) => {
    #[doc = $doc]
    pub struct $name<'a, T>(Merge<'a, T>);

    impl<'a, T: Ord> Iterator for $name<'a, T> {
      type Item = &'a T;

      #[inline]
      fn next(&mut self) -> Option<&'a T> {
        self.0.next()
      }
    }

    impl<T: Ord> FusedIterator for $name<'_, T> {}
  };
}

set_iter!(Union, "Iterator over the union of two [`SkipSet`]s.");
set_iter!(Intersection, "Iterator over the intersection of two [`SkipSet`]s.");
set_iter!(Difference, "Iterator over the difference of two [`SkipSet`]s.");
set_iter!(
  SymmetricDifference,
  "Iterator over the symmetric difference of two [`SkipSet`]s."
);
