use crate::raw::{Cursor, IntoNodes, Nodes, Path, RawList};
use std::borrow::Borrow;
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::iter::FusedIterator;
use std::marker::PhantomData;
use std::mem;
use std::ops::{Bound, Index, RangeBounds};

#[cfg(test)]
mod tests;

/// An ordered map backed by a skiplist.
///
/// The API follows [`std::collections::BTreeMap`].
pub struct SkipMap<K, V> {
  raw: RawList<K, V>,
}

impl<K, V> SkipMap<K, V> {
  /// Makes a new, empty map.
  pub fn new() -> Self {
    SkipMap { raw: RawList::new() }
  }

  /// Returns the number of elements in the map.
  #[inline]
  pub fn len(&self) -> usize {
    self.raw.len()
  }

  /// Returns `true` if the map contains no elements.
  #[inline]
  pub fn is_empty(&self) -> bool {
    self.raw.len() == 0
  }

  /// Removes every element.
  pub fn clear(&mut self) {
    self.raw.clear()
  }

  /// Returns the first key-value pair, the one with the smallest key.
  pub fn first_key_value(&self) -> Option<(&K, &V)> {
    unsafe { self.raw.first().pair() }
  }

  /// Returns the last key-value pair, the one with the largest key.
  pub fn last_key_value(&self) -> Option<(&K, &V)> {
    unsafe { self.raw.last().pair() }
  }

  /// Removes and returns the first element.
  pub fn pop_first(&mut self) -> Option<(K, V)> {
    self.raw.pop_first()
  }

  /// Removes and returns the last element.
  pub fn pop_last(&mut self) -> Option<(K, V)> {
    self.raw.pop_last()
  }

  #[inline]
  fn find<Q>(&self, key: &Q) -> Cursor<K, V>
  where
    K: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    unsafe { self.raw.find(|k| k.borrow().cmp(key)) }
  }

  /// Returns a reference to the value for `key`.
  pub fn get<Q>(&self, key: &Q) -> Option<&V>
  where
    K: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    unsafe { self.find(key).pair().map(|(_, v)| v) }
  }

  /// Returns the stored key and its value for `key`.
  pub fn get_key_value<Q>(&self, key: &Q) -> Option<(&K, &V)>
  where
    K: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    unsafe { self.find(key).pair() }
  }

  /// Returns a mutable reference to the value for `key`.
  pub fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V>
  where
    K: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    unsafe { self.find(key).pair_mut().map(|(_, v)| v) }
  }

  /// Returns `true` if the map contains `key`.
  pub fn contains_key<Q>(&self, key: &Q) -> bool
  where
    K: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    !self.find(key).is_null()
  }

  /// Inserts a key-value pair. If the key was present its value is replaced and the old value
  /// returned; the stored key is kept.
  pub fn insert(&mut self, key: K, value: V) -> Option<V>
  where
    K: Ord,
  {
    unsafe {
      let mut path = self.raw.path();
      match self.raw.search(|k| k.cmp(&key), &mut path) {
        Some(c) => Some(mem::replace(c.value_mut(), value)),
        None => {
          self.raw.insert_at(&mut path, key, value);
          None
        }
      }
    }
  }

  /// Inserts `key` with a default value. If an equal key is stored, replaces it and returns the
  /// old key.
  pub(crate) fn replace_key(&mut self, key: K) -> Option<K>
  where
    K: Ord,
    V: Default,
  {
    unsafe {
      let mut path = self.raw.path();
      match self.raw.search(|k| k.cmp(&key), &mut path) {
        Some(c) => Some(mem::replace(c.key_mut(), key)),
        None => {
          self.raw.insert_at(&mut path, key, V::default());
          None
        }
      }
    }
  }

  /// Removes `key` and returns its value.
  pub fn remove<Q>(&mut self, key: &Q) -> Option<V>
  where
    K: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    self.remove_entry(key).map(|(_, v)| v)
  }

  /// Removes `key` and returns the stored key and its value.
  pub fn remove_entry<Q>(&mut self, key: &Q) -> Option<(K, V)>
  where
    K: Borrow<Q>,
    Q: Ord + ?Sized,
  {
    unsafe {
      let mut path = self.raw.path();
      self.raw.search(|k| k.borrow().cmp(key), &mut path)?;
      Some(self.raw.remove_at(&mut path))
    }
  }

  /// Gets the entry for `key` for in-place manipulation.
  pub fn entry(&mut self, key: K) -> Entry<'_, K, V>
  where
    K: Ord,
  {
    unsafe {
      let mut path = self.raw.path();
      match self.raw.search(|k| k.cmp(&key), &mut path) {
        Some(cursor) => Entry::Occupied(OccupiedEntry { map: self, cursor }),
        None => Entry::Vacant(VacantEntry { map: self, key, path }),
      }
    }
  }

  /// Keeps only the elements for which `f` returns `true`, visiting them in key order.
  pub fn retain<F>(&mut self, f: F)
  where
    F: FnMut(&K, &mut V) -> bool,
  {
    self.raw.retain(f)
  }

  /// Splits the map in two at `key`: `self` keeps the keys below it and the returned map gets the
  /// rest.
  pub fn split_off<Q>(&mut self, key: &Q) -> Self
  where
    K: Borrow<Q> + Ord,
    Q: Ord + ?Sized,
  {
    unsafe {
      let mut path = self.raw.path();
      self.raw.path_where(|k| k.borrow() < key, &mut path);
      SkipMap {
        raw: self.raw.split_off(&mut path),
      }
    }
  }

  /// Moves every element of `other` into `self`, leaving `other` empty. A key present in both
  /// keeps the value from `other`.
  pub fn append(&mut self, other: &mut Self)
  where
    K: Ord,
  {
    if other.is_empty() {
      return;
    }
    if self.is_empty() {
      mem::swap(self, other);
      return;
    }
    unsafe {
      if self.raw.last().key() < other.raw.first().key() {
        self.raw.append(&mut other.raw);
        return;
      }
    }
    let a = mem::replace(&mut self.raw, RawList::new()).into_nodes();
    let b = mem::replace(&mut other.raw, RawList::new()).into_nodes();
    self.raw = RawList::from_ordered(merge_unique(a, b));
  }

  /// Builds a map from pairs in strictly increasing key order.
  pub(crate) fn from_sorted(pairs: impl Iterator<Item = (K, V)>) -> Self {
    SkipMap {
      raw: RawList::from_ordered(pairs),
    }
  }

  /// Iterates over the entries in key order.
  pub fn iter(&self) -> Iter<'_, K, V> {
    Iter {
      nodes: self.raw.nodes(),
      marker: PhantomData,
    }
  }

  /// Iterates over the entries in key order with mutable values.
  pub fn iter_mut(&mut self) -> IterMut<'_, K, V> {
    IterMut {
      nodes: self.raw.nodes(),
      marker: PhantomData,
    }
  }

  /// Iterates over the keys in order.
  pub fn keys(&self) -> Keys<'_, K, V> {
    Keys(self.iter())
  }

  /// Iterates over the values in key order.
  pub fn values(&self) -> Values<'_, K, V> {
    Values(self.iter())
  }

  /// Iterates over the values in key order, mutably.
  pub fn values_mut(&mut self) -> ValuesMut<'_, K, V> {
    ValuesMut(self.iter_mut())
  }

  /// Consumes the map, yielding the keys in order.
  pub fn into_keys(self) -> IntoKeys<K, V> {
    IntoKeys(self.into_iter())
  }

  /// Consumes the map, yielding the values in key order.
  pub fn into_values(self) -> IntoValues<K, V> {
    IntoValues(self.into_iter())
  }

  unsafe fn range_nodes<T, R>(&self, range: &R) -> Nodes<K, V>
  where
    T: Ord + ?Sized,
    K: Borrow<T> + Ord,
    R: RangeBounds<T>,
  {
    match (range.start_bound(), range.end_bound()) {
      (Bound::Excluded(s), Bound::Excluded(e)) if s == e => {
        panic!("range start and end are equal and excluded in SkipMap")
      }
      (Bound::Included(s) | Bound::Excluded(s), Bound::Included(e) | Bound::Excluded(e)) if s > e => {
        panic!("range start is greater than range end in SkipMap")
      }
      _ => {}
    }
    unsafe {
      let (front, fpos) = match range.start_bound() {
        Bound::Unbounded => (self.raw.first(), 0),
        Bound::Included(s) => self.raw.lower_bound(|k| k.borrow() < s),
        Bound::Excluded(s) => self.raw.lower_bound(|k| k.borrow() <= s),
      };
      if front.is_null() {
        return Nodes::EMPTY;
      }
      let (end, epos) = match range.end_bound() {
        Bound::Unbounded => (Cursor::NONE, self.raw.len()),
        Bound::Included(e) => self.raw.lower_bound(|k| k.borrow() <= e),
        Bound::Excluded(e) => self.raw.lower_bound(|k| k.borrow() < e),
      };
      if epos <= fpos {
        return Nodes::EMPTY;
      }
      Nodes {
        front,
        back: self.raw.before(end),
        len: epos - fpos,
      }
    }
  }

  /// Iterates over the entries whose keys fall in `range`.
  ///
  /// # Panics
  ///
  /// Panics if the start is greater than the end. Panics if both are equal and excluded.
  pub fn range<T, R>(&self, range: R) -> Range<'_, K, V>
  where
    T: Ord + ?Sized,
    K: Borrow<T> + Ord,
    R: RangeBounds<T>,
  {
    Range(Iter {
      nodes: unsafe { self.range_nodes(&range) },
      marker: PhantomData,
    })
  }

  /// Iterates over the entries whose keys fall in `range`, with mutable values.
  ///
  /// # Panics
  ///
  /// Panics if the start is greater than the end. Panics if both are equal and excluded.
  pub fn range_mut<T, R>(&mut self, range: R) -> RangeMut<'_, K, V>
  where
    T: Ord + ?Sized,
    K: Borrow<T> + Ord,
    R: RangeBounds<T>,
  {
    RangeMut(IterMut {
      nodes: unsafe { self.range_nodes(&range) },
      marker: PhantomData,
    })
  }

  #[cfg(test)]
  pub(crate) fn check_invariants(&self)
  where
    K: Ord,
  {
    self.raw.check_invariants(|a, b| a < b)
  }
}

/// Merges two strictly increasing runs into one. On equal keys the pair from `b` is kept.
fn merge_unique<K: Ord, V>(a: IntoNodes<K, V>, b: IntoNodes<K, V>) -> impl Iterator<Item = (K, V)> {
  let mut a = a;
  let mut b = b;
  let mut a = std::iter::from_fn(move || a.next()).peekable();
  let mut b = std::iter::from_fn(move || b.next()).peekable();
  std::iter::from_fn(move || {
    let ord = match (a.peek(), b.peek()) {
      (None, None) => return None,
      (Some(_), None) => return a.next(),
      (None, Some(_)) => return b.next(),
      (Some(x), Some(y)) => x.0.cmp(&y.0),
    };
    match ord {
      Ordering::Less => a.next(),
      Ordering::Greater => b.next(),
      Ordering::Equal => {
        a.next();
        b.next()
      }
    }
  })
}

/// A view into one entry of a [`SkipMap`], which may be vacant or occupied.
#[allow(clippy::large_enum_variant)]
pub enum Entry<'a, K, V> {
  /// The key is not in the map.
  Vacant(VacantEntry<'a, K, V>),
  /// The key is in the map.
  Occupied(OccupiedEntry<'a, K, V>),
}

/// A view into a key that is not in the map.
pub struct VacantEntry<'a, K, V> {
  map: &'a mut SkipMap<K, V>,
  key: K,
  path: Path<K, V>,
}

/// A view into a key that is in the map.
pub struct OccupiedEntry<'a, K, V> {
  map: &'a mut SkipMap<K, V>,
  cursor: Cursor<K, V>,
}

unsafe impl<K: Send, V: Send> Send for VacantEntry<'_, K, V> {}
unsafe impl<K: Sync, V: Sync> Sync for VacantEntry<'_, K, V> {}
unsafe impl<K: Send, V: Send> Send for OccupiedEntry<'_, K, V> {}
unsafe impl<K: Sync, V: Sync> Sync for OccupiedEntry<'_, K, V> {}

impl<'a, K, V> Entry<'a, K, V> {
  /// Inserts `default` if the entry is vacant and returns a mutable reference to the value.
  pub fn or_insert(self, default: V) -> &'a mut V {
    match self {
      Entry::Occupied(e) => e.into_mut(),
      Entry::Vacant(e) => e.insert(default),
    }
  }

  /// Inserts the result of `default` if the entry is vacant and returns a mutable reference to the
  /// value.
  pub fn or_insert_with<F: FnOnce() -> V>(self, default: F) -> &'a mut V {
    match self {
      Entry::Occupied(e) => e.into_mut(),
      Entry::Vacant(e) => e.insert(default()),
    }
  }

  /// Like [`or_insert_with`](Self::or_insert_with), passing the key to `default`.
  pub fn or_insert_with_key<F: FnOnce(&K) -> V>(self, default: F) -> &'a mut V {
    match self {
      Entry::Occupied(e) => e.into_mut(),
      Entry::Vacant(e) => {
        let value = default(&e.key);
        e.insert(value)
      }
    }
  }

  /// Inserts the default value if the entry is vacant and returns a mutable reference to the value.
  pub fn or_default(self) -> &'a mut V
  where
    V: Default,
  {
    self.or_insert_with(V::default)
  }

  /// Returns the entry's key.
  pub fn key(&self) -> &K {
    match self {
      Entry::Occupied(e) => e.key(),
      Entry::Vacant(e) => e.key(),
    }
  }

  /// Applies `f` to the value if the entry is occupied.
  pub fn and_modify<F: FnOnce(&mut V)>(self, f: F) -> Self {
    match self {
      Entry::Occupied(mut e) => {
        f(e.get_mut());
        Entry::Occupied(e)
      }
      Entry::Vacant(e) => Entry::Vacant(e),
    }
  }
}

impl<'a, K, V> VacantEntry<'a, K, V> {
  /// Returns the key that would be inserted.
  pub fn key(&self) -> &K {
    &self.key
  }

  /// Takes ownership of the key.
  pub fn into_key(self) -> K {
    self.key
  }

  /// Inserts `value` and returns a mutable reference to it.
  pub fn insert(self, value: V) -> &'a mut V {
    self.insert_key_value(value).1
  }

  pub(crate) fn insert_key_value(self, value: V) -> (&'a K, &'a mut V) {
    let VacantEntry { map, key, mut path } = self;
    unsafe {
      let c = map.raw.insert_at(&mut path, key, value);
      (c.key(), c.value_mut())
    }
  }
}

impl<'a, K, V> OccupiedEntry<'a, K, V> {
  /// Returns the stored key.
  pub fn key(&self) -> &K {
    unsafe { self.cursor.key() }
  }

  /// Returns the value.
  pub fn get(&self) -> &V {
    unsafe { self.cursor.value() }
  }

  /// Returns the value mutably.
  pub fn get_mut(&mut self) -> &mut V {
    unsafe { self.cursor.value_mut() }
  }

  /// Converts the entry into a mutable reference to the value with the map's lifetime.
  pub fn into_mut(self) -> &'a mut V {
    unsafe { self.cursor.value_mut() }
  }

  /// Replaces the value and returns the old one.
  pub fn insert(&mut self, value: V) -> V {
    mem::replace(self.get_mut(), value)
  }
}

impl<K: Ord, V> OccupiedEntry<'_, K, V> {
  /// Removes the entry and returns its value.
  pub fn remove(self) -> V {
    self.remove_entry().1
  }

  /// Removes the entry and returns the stored key and its value.
  pub fn remove_entry(self) -> (K, V) {
    unsafe {
      let key: *const K = self.cursor.key();
      let mut path = self.map.raw.path();
      self.map.raw.search(|k| k.cmp(&*key), &mut path);
      self.map.raw.remove_at(&mut path)
    }
  }
}

impl<K, V> Default for SkipMap<K, V> {
  fn default() -> Self {
    SkipMap::new()
  }
}

impl<K: Clone, V: Clone> Clone for SkipMap<K, V> {
  fn clone(&self) -> Self {
    SkipMap::from_sorted(self.iter().map(|(k, v)| (k.clone(), v.clone())))
  }
}

impl<K: Ord, V> FromIterator<(K, V)> for SkipMap<K, V> {
  fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
    let mut pairs: Vec<(K, V)> = iter.into_iter().collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    let mut pairs = pairs.into_iter().peekable();
    let dedup = std::iter::from_fn(move || {
      loop {
        let pair = pairs.next()?;
        match pairs.peek() {
          Some(next) if next.0 == pair.0 => continue,
          _ => return Some(pair),
        }
      }
    });
    SkipMap::from_sorted(dedup)
  }
}

impl<K: Ord, V, const N: usize> From<[(K, V); N]> for SkipMap<K, V> {
  fn from(pairs: [(K, V); N]) -> Self {
    pairs.into_iter().collect()
  }
}

impl<K: Ord, V> Extend<(K, V)> for SkipMap<K, V> {
  fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, iter: I) {
    for (k, v) in iter {
      self.insert(k, v);
    }
  }
}

impl<'a, K: Ord + Copy, V: Copy> Extend<(&'a K, &'a V)> for SkipMap<K, V> {
  fn extend<I: IntoIterator<Item = (&'a K, &'a V)>>(&mut self, iter: I) {
    for (k, v) in iter {
      self.insert(*k, *v);
    }
  }
}

impl<K: PartialEq, V: PartialEq> PartialEq for SkipMap<K, V> {
  fn eq(&self, other: &Self) -> bool {
    self.len() == other.len() && self.iter().eq(other.iter())
  }
}

impl<K: Eq, V: Eq> Eq for SkipMap<K, V> {}

impl<K: PartialOrd, V: PartialOrd> PartialOrd for SkipMap<K, V> {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    self.iter().partial_cmp(other.iter())
  }
}

impl<K: Ord, V: Ord> Ord for SkipMap<K, V> {
  fn cmp(&self, other: &Self) -> Ordering {
    self.iter().cmp(other.iter())
  }
}

impl<K: Hash, V: Hash> Hash for SkipMap<K, V> {
  fn hash<H: Hasher>(&self, state: &mut H) {
    state.write_usize(self.len());
    for entry in self.iter() {
      entry.hash(state);
    }
  }
}

impl<K: fmt::Debug, V: fmt::Debug> fmt::Debug for SkipMap<K, V> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_map().entries(self.iter()).finish()
  }
}

impl<K, Q, V> Index<&Q> for SkipMap<K, V>
where
  K: Borrow<Q>,
  Q: Ord + ?Sized,
{
  type Output = V;

  /// # Panics
  ///
  /// Panics if the key is not present.
  fn index(&self, key: &Q) -> &V {
    self.get(key).expect("no entry found for key")
  }
}

impl<'a, K, V> IntoIterator for &'a SkipMap<K, V> {
  type Item = (&'a K, &'a V);
  type IntoIter = Iter<'a, K, V>;

  fn into_iter(self) -> Iter<'a, K, V> {
    self.iter()
  }
}

impl<'a, K, V> IntoIterator for &'a mut SkipMap<K, V> {
  type Item = (&'a K, &'a mut V);
  type IntoIter = IterMut<'a, K, V>;

  fn into_iter(self) -> IterMut<'a, K, V> {
    self.iter_mut()
  }
}

impl<K, V> IntoIterator for SkipMap<K, V> {
  type Item = (K, V);
  type IntoIter = IntoIter<K, V>;

  fn into_iter(self) -> IntoIter<K, V> {
    IntoIter(self.raw.into_nodes())
  }
}

/// Iterator over the entries of a [`SkipMap`] in key order.
pub struct Iter<'a, K, V> {
  nodes: Nodes<K, V>,
  marker: PhantomData<(&'a K, &'a V)>,
}

unsafe impl<K: Sync, V: Sync> Send for Iter<'_, K, V> {}
unsafe impl<K: Sync, V: Sync> Sync for Iter<'_, K, V> {}

impl<K, V> Clone for Iter<'_, K, V> {
  fn clone(&self) -> Self {
    Iter {
      nodes: self.nodes,
      marker: PhantomData,
    }
  }
}

impl<'a, K, V> Iterator for Iter<'a, K, V> {
  type Item = (&'a K, &'a V);

  #[inline]
  fn next(&mut self) -> Option<Self::Item> {
    self.nodes.next().map(|c| unsafe { (c.key(), c.value()) })
  }

  #[inline]
  fn size_hint(&self) -> (usize, Option<usize>) {
    (self.nodes.len, Some(self.nodes.len))
  }
}

impl<K, V> DoubleEndedIterator for Iter<'_, K, V> {
  #[inline]
  fn next_back(&mut self) -> Option<Self::Item> {
    self.nodes.next_back().map(|c| unsafe { (c.key(), c.value()) })
  }
}

impl<K, V> ExactSizeIterator for Iter<'_, K, V> {}
impl<K, V> FusedIterator for Iter<'_, K, V> {}

/// Iterator over the entries of a [`SkipMap`] in key order with mutable values.
pub struct IterMut<'a, K, V> {
  nodes: Nodes<K, V>,
  marker: PhantomData<(&'a K, &'a mut V)>,
}

unsafe impl<K: Sync, V: Send> Send for IterMut<'_, K, V> {}
unsafe impl<K: Sync, V: Sync> Sync for IterMut<'_, K, V> {}

impl<'a, K, V> Iterator for IterMut<'a, K, V> {
  type Item = (&'a K, &'a mut V);

  #[inline]
  fn next(&mut self) -> Option<Self::Item> {
    self.nodes.next().map(|c| unsafe { (c.key(), c.value_mut()) })
  }

  #[inline]
  fn size_hint(&self) -> (usize, Option<usize>) {
    (self.nodes.len, Some(self.nodes.len))
  }
}

impl<K, V> DoubleEndedIterator for IterMut<'_, K, V> {
  #[inline]
  fn next_back(&mut self) -> Option<Self::Item> {
    self.nodes.next_back().map(|c| unsafe { (c.key(), c.value_mut()) })
  }
}

impl<K, V> ExactSizeIterator for IterMut<'_, K, V> {}
impl<K, V> FusedIterator for IterMut<'_, K, V> {}

/// Owning iterator over the entries of a [`SkipMap`] in key order.
pub struct IntoIter<K, V>(IntoNodes<K, V>);

impl<K, V> Iterator for IntoIter<K, V> {
  type Item = (K, V);

  #[inline]
  fn next(&mut self) -> Option<(K, V)> {
    self.0.next()
  }

  #[inline]
  fn size_hint(&self) -> (usize, Option<usize>) {
    (self.0.len(), Some(self.0.len()))
  }
}

impl<K, V> DoubleEndedIterator for IntoIter<K, V> {
  #[inline]
  fn next_back(&mut self) -> Option<(K, V)> {
    self.0.next_back()
  }
}

impl<K, V> ExactSizeIterator for IntoIter<K, V> {}
impl<K, V> FusedIterator for IntoIter<K, V> {}

macro_rules! projection {
  ($name:ident, $inner:ident, $item:ty, $pat:pat => $out:expr, [$($lt:lifetime)?], $doc:literal) => {
    #[doc = $doc]
    pub struct $name<$($lt,)? K, V>($inner<$($lt,)? K, V>);

    impl<$($lt,)? K, V> Iterator for $name<$($lt,)? K, V> {
      type Item = $item;

      #[inline]
      fn next(&mut self) -> Option<$item> {
        self.0.next().map(|$pat| $out)
      }

      #[inline]
      fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
      }
    }

    impl<$($lt,)? K, V> DoubleEndedIterator for $name<$($lt,)? K, V> {
      #[inline]
      fn next_back(&mut self) -> Option<$item> {
        self.0.next_back().map(|$pat| $out)
      }
    }

    impl<$($lt,)? K, V> ExactSizeIterator for $name<$($lt,)? K, V> {}
    impl<$($lt,)? K, V> FusedIterator for $name<$($lt,)? K, V> {}
  };
}

projection!(Range, Iter, (&'a K, &'a V), e => e, ['a], "Iterator over a sub-range of a [`SkipMap`].");
projection!(RangeMut, IterMut, (&'a K, &'a mut V), e => e, ['a], "Iterator over a sub-range of a [`SkipMap`] with mutable values.");
projection!(Keys, Iter, &'a K, (k, _) => k, ['a], "Iterator over the keys of a [`SkipMap`].");
projection!(Values, Iter, &'a V, (_, v) => v, ['a], "Iterator over the values of a [`SkipMap`].");
projection!(ValuesMut, IterMut, &'a mut V, (_, v) => v, ['a], "Iterator over the values of a [`SkipMap`], mutably.");
projection!(IntoKeys, IntoIter, K, (k, _) => k, [], "Owning iterator over the keys of a [`SkipMap`].");
projection!(IntoValues, IntoIter, V, (_, v) => v, [], "Owning iterator over the values of a [`SkipMap`].");

impl<K, V> Clone for Range<'_, K, V> {
  fn clone(&self) -> Self {
    Range(self.0.clone())
  }
}
