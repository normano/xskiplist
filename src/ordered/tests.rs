use super::OrderedSkipList;
use crate::test_util::scaled;
use std::cmp::Ordering;

struct Rng(u64);

impl Rng {
  fn next(&mut self) -> u64 {
    self.0 ^= self.0 << 13;
    self.0 ^= self.0 >> 7;
    self.0 ^= self.0 << 17;
    self.0
  }

  fn below(&mut self, n: u64) -> u64 {
    self.next() % n
  }
}

/// Ordered by `key` only, so equal elements stay distinguishable by `tag`.
#[derive(Clone, Copy, Debug)]
struct Item {
  key: u64,
  tag: u64,
}

impl PartialEq for Item {
  fn eq(&self, other: &Self) -> bool {
    self.key == other.key
  }
}

impl Eq for Item {}

impl PartialOrd for Item {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(other))
  }
}

impl Ord for Item {
  fn cmp(&self, other: &Self) -> Ordering {
    self.key.cmp(&other.key)
  }
}

fn tags(items: impl Iterator<Item = Item>) -> Vec<(u64, u64)> {
  items.map(|i| (i.key, i.tag)).collect()
}

fn assert_same(list: &OrderedSkipList<Item>, oracle: &[Item]) {
  list.check_invariants();
  assert_eq!(list.len(), oracle.len());
  assert_eq!(tags(list.iter().copied()), tags(oracle.iter().copied()));
  assert_eq!(tags(list.iter().rev().copied()), tags(oracle.iter().rev().copied()));
  for (i, item) in oracle.iter().enumerate() {
    assert_eq!(list.get_by_index(i).map(|x| x.tag), Some(item.tag));
  }
  assert_eq!(list.get_by_index(oracle.len()), None);
}

fn probe(key: u64) -> Item {
  Item { key, tag: u64::MAX }
}

#[test]
fn random_operations_match_sorted_vec() {
  for key_space in [8u64, 300] {
    let mut rng = Rng(0xD00D ^ key_space);
    let mut list = OrderedSkipList::new();
    let mut oracle: Vec<Item> = Vec::new();
    for step in 0..scaled(8000u64) {
      let key = rng.below(key_space);
      let p = probe(key);
      let lower = oracle.partition_point(|x| x.key < key);
      let upper = oracle.partition_point(|x| x.key <= key);
      match rng.below(11) {
        0..=3 => {
          let item = Item { key, tag: step };
          list.insert(item);
          oracle.insert(upper, item);
        }
        4 => {
          let expected = (lower < upper).then(|| oracle.remove(lower));
          assert_eq!(list.take(&p).map(|x| x.tag), expected.map(|x| x.tag));
        }
        5 => {
          let expected = (lower < upper).then(|| oracle.remove(upper - 1));
          assert_eq!(list.take_last(&p).map(|x| x.tag), expected.map(|x| x.tag));
        }
        6 => {
          assert_eq!(list.contains(&p), lower < upper);
          assert_eq!(list.rank(&p), (lower < upper).then_some(lower));
          assert_eq!(list.count(&p), upper - lower);
        }
        7 => {
          assert_eq!(
            list.pop_first().map(|x| x.tag),
            (!oracle.is_empty()).then(|| oracle.remove(0).tag)
          );
          assert_eq!(list.pop_last().map(|x| x.tag), oracle.pop().map(|x| x.tag));
        }
        8 if !oracle.is_empty() => {
          let at = rng.below(oracle.len() as u64) as usize;
          assert_eq!(list.remove(at).tag, oracle.remove(at).tag);
        }
        9 => {
          let mut tail = list.split_off(&p);
          let oracle_tail = oracle.split_off(lower);
          tail.check_invariants();
          assert_eq!(tags(tail.iter().copied()), tags(oracle_tail.iter().copied()));
          list.append(&mut tail);
          assert!(tail.is_empty());
          oracle.extend(oracle_tail);
        }
        _ => {
          let range: Vec<_> = tags(list.range(probe(key)..probe(key + 3)).copied());
          let expected = oracle.iter().filter(|x| x.key >= key && x.key < key + 3).copied();
          assert_eq!(range, tags(expected));
        }
      }
      if step % 97 == 0 {
        assert_same(&list, &oracle);
      }
    }
    assert_same(&list, &oracle);
  }
}

#[test]
fn append_merges_interleaved_lists() {
  let mut a: OrderedSkipList<u64> = (0..100).map(|v| v * 2).collect();
  let mut b: OrderedSkipList<u64> = (0..100).map(|v| v * 2 + 1).chain([4, 4]).collect();
  a.append(&mut b);
  a.check_invariants();
  b.check_invariants();
  assert!(b.is_empty());
  let mut expected: Vec<u64> = (0..200).chain([4, 4]).collect();
  expected.sort();
  assert!(a.iter().copied().eq(expected));
  assert_eq!(a.count(&4), 3);
}

#[test]
fn split_off_index_and_retain() {
  let mut list: OrderedSkipList<u64> = (0..1000).rev().collect();
  let tail = list.split_off_index(400);
  assert!(list.iter().copied().eq(0..400));
  assert!(tail.iter().copied().eq(400..1000));
  tail.check_invariants();
  list.retain(|v| v % 3 == 0);
  list.check_invariants();
  assert!(list.iter().copied().eq((0..400).filter(|v| v % 3 == 0)));
  assert_eq!(list.first(), Some(&0));
  assert_eq!(list.last(), Some(&399));
  let clone = list.clone();
  assert_eq!(clone, list);
  assert_eq!(format!("{clone:?}"), format!("{:?}", clone.iter().collect::<Vec<_>>()));
  list.clear();
  assert!(list.is_empty());
}
