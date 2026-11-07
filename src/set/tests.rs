use super::SkipSet;
use crate::test_util::scaled;
use std::collections::BTreeSet;
use std::ops::Bound;

struct Rng(u64);

impl Rng {
  fn below(&mut self, n: u64) -> u64 {
    self.0 ^= self.0 << 13;
    self.0 ^= self.0 >> 7;
    self.0 ^= self.0 << 17;
    self.0 % n
  }
}

fn assert_same(set: &SkipSet<u64>, oracle: &BTreeSet<u64>) {
  set.check_invariants();
  assert_eq!(set.len(), oracle.len());
  assert!(set.iter().eq(oracle.iter()));
  assert!(set.iter().rev().eq(oracle.iter().rev()));
  assert_eq!(set.first(), oracle.first());
  assert_eq!(set.last(), oracle.last());
}

#[test]
fn random_operations_match_btreeset() {
  let mut rng = Rng(0x5eed);
  let mut set = SkipSet::new();
  let mut oracle = BTreeSet::new();
  for step in 0..scaled(20_000) {
    let v = rng.below(512);
    match rng.below(8) {
      0..=2 => assert_eq!(set.insert(v), oracle.insert(v)),
      3 | 4 => assert_eq!(set.remove(&v), oracle.remove(&v)),
      5 => assert_eq!(set.take(&v), oracle.take(&v)),
      6 => assert_eq!(set.contains(&v), oracle.contains(&v)),
      _ => {
        if rng.below(2) == 0 {
          assert_eq!(set.pop_first(), oracle.pop_first());
        } else {
          assert_eq!(set.pop_last(), oracle.pop_last());
        }
      }
    }
    assert_eq!(set.get(&v), oracle.get(&v));
    if step % 251 == 0 {
      assert_same(&set, &oracle);
    }
  }
  assert_same(&set, &oracle);
}

#[test]
fn ranges_retain_and_conversions_match_btreeset() {
  let set: SkipSet<u64> = (0..100).rev().chain(0..50).collect();
  let oracle: BTreeSet<u64> = (0..100).rev().chain(0..50).collect();
  assert_same(&set, &oracle);
  let range = (Bound::Excluded(10), Bound::Included(40));
  assert!(set.range(range).eq(oracle.range(range)));
  assert!(set.range(range).rev().eq(oracle.range(range).rev()));
  let mut set = set;
  let mut oracle = oracle;
  set.retain(|v| v % 7 != 0);
  oracle.retain(|v| v % 7 != 0);
  assert_same(&set, &oracle);
  set.extend([200, 3, 7]);
  oracle.extend([200, 3, 7]);
  assert_same(&set, &oracle);
  assert!(set.clone().into_iter().eq(oracle.clone().into_iter()));
  assert!((&set).into_iter().eq(&oracle));
  assert_eq!(format!("{set:?}"), format!("{oracle:?}"));
  assert_eq!(set, set.clone());
  assert_eq!(
    SkipSet::from([3u64, 1, 2]).into_iter().collect::<Vec<_>>(),
    vec![1, 2, 3]
  );
  set.clear();
  assert!(set.is_empty());
  assert_eq!(SkipSet::<u64>::default().len(), 0);
}

#[test]
fn borrowed_lookups() {
  let mut set = SkipSet::new();
  set.insert(String::from("x"));
  assert!(set.contains("x"));
  assert_eq!(set.get("x").map(String::as_str), Some("x"));
  assert!(set.remove("x"));
  assert!(!set.contains("x"));
}

#[test]
fn set_algebra_matches_btreeset() {
  let a: SkipSet<u64> = (0..300).filter(|v| v % 3 == 0).collect();
  let b: SkipSet<u64> = (0..300).filter(|v| v % 5 == 0).collect();
  let oa: BTreeSet<u64> = a.iter().copied().collect();
  let ob: BTreeSet<u64> = b.iter().copied().collect();
  assert!(a.union(&b).eq(oa.union(&ob)));
  assert!(a.intersection(&b).eq(oa.intersection(&ob)));
  assert!(a.difference(&b).eq(oa.difference(&ob)));
  assert!(a.symmetric_difference(&b).eq(oa.symmetric_difference(&ob)));
  assert!((&a | &b).iter().eq((&oa | &ob).iter()));
  assert!((&a & &b).iter().eq((&oa & &ob).iter()));
  assert!((&a - &b).iter().eq((&oa - &ob).iter()));
  assert!((&a ^ &b).iter().eq((&oa ^ &ob).iter()));
  (&a | &b).check_invariants();
}

#[test]
fn replace_entry_split_append() {
  use super::Entry;
  let mut set: SkipSet<String> = ["a", "b", "c"].into_iter().map(String::from).collect();
  assert_eq!(set.replace(String::from("b")), Some(String::from("b")));
  assert_eq!(set.replace(String::from("d")), None);
  match set.entry(String::from("a")) {
    Entry::Occupied(e) => assert_eq!(e.get(), "a"),
    Entry::Vacant(_) => panic!("present"),
  }
  match set.entry(String::from("e")) {
    Entry::Vacant(e) => assert_eq!(e.insert(), "e"),
    Entry::Occupied(_) => panic!("absent"),
  }
  match set.entry(String::from("c")) {
    Entry::Occupied(e) => assert_eq!(e.remove(), "c"),
    Entry::Vacant(_) => panic!("present"),
  }
  let mut tail = set.split_off("b");
  assert!(set.iter().eq(["a"].iter()));
  assert!(tail.iter().eq(["b", "d", "e"].iter()));
  set.append(&mut tail);
  set.check_invariants();
  assert!(tail.is_empty());
  assert_eq!(set.len(), 4);
}
