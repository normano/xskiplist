use super::SkipList;
use std::cell::Cell;
use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

struct Rng(u64);

impl Rng {
  fn next(&mut self) -> u64 {
    self.0 ^= self.0 << 13;
    self.0 ^= self.0 >> 7;
    self.0 ^= self.0 << 17;
    self.0
  }

  fn below(&mut self, n: usize) -> usize {
    (self.next() % n as u64) as usize
  }
}

fn assert_same(list: &SkipList<u64>, oracle: &VecDeque<u64>) {
  list.check_invariants();
  assert_eq!(list.len(), oracle.len());
  assert!(list.iter().eq(oracle.iter()));
  assert!(list.iter().rev().eq(oracle.iter().rev()));
  assert_eq!(list.front(), oracle.front());
  assert_eq!(list.back(), oracle.back());
  for i in 0..oracle.len() {
    assert_eq!(list.get(i), oracle.get(i), "index {i}");
  }
  assert_eq!(list.get(oracle.len()), None);
}

#[test]
fn random_operations_match_vecdeque() {
  for seed in 1..=6u64 {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    let mut list = SkipList::new();
    let mut oracle = VecDeque::new();
    for step in 0..6000 {
      let value = rng.next();
      let len = oracle.len();
      match rng.below(12) {
        0 | 1 => {
          list.push_back(value);
          oracle.push_back(value);
        }
        2 | 3 => {
          list.push_front(value);
          oracle.push_front(value);
        }
        4 => assert_eq!(list.pop_front(), oracle.pop_front()),
        5 => assert_eq!(list.pop_back(), oracle.pop_back()),
        6 | 7 => {
          let at = rng.below(len + 1);
          list.insert(at, value);
          oracle.insert(at, value);
        }
        8 if len > 0 => {
          let at = rng.below(len);
          assert_eq!(list.remove(at), oracle.remove(at).unwrap());
        }
        9 if len > 0 => {
          let at = rng.below(len);
          *list.get_mut(at).unwrap() = value;
          oracle[at] = value;
        }
        10 => {
          let at = rng.below(len + 1);
          let mut tail = list.split_off(at);
          let oracle_tail = oracle.split_off(at);
          tail.check_invariants();
          assert!(tail.iter().eq(oracle_tail.iter()));
          list.check_invariants();
          if rng.below(2) == 0 {
            list.append(&mut tail);
            assert!(tail.is_empty());
            tail.check_invariants();
            oracle.extend(oracle_tail);
          } else {
            oracle = oracle_tail;
            list = tail;
          }
        }
        _ => {
          let modulus = 2 + rng.below(5) as u64;
          list.retain(|v| v % modulus != 0);
          oracle.retain(|v| v % modulus != 0);
        }
      }
      if step % 101 == 0 {
        assert_same(&list, &oracle);
      }
    }
    assert_same(&list, &oracle);
  }
}

#[test]
fn split_off_and_append_at_edges() {
  let mut list: SkipList<u64> = (0..1000).collect();
  let mut tail = list.split_off(1000);
  assert!(tail.is_empty());
  list.append(&mut tail);
  let mut all = list.split_off(0);
  assert!(list.is_empty());
  list.check_invariants();
  all.check_invariants();
  list.append(&mut all);
  assert!(list.iter().copied().eq(0..1000));
  for _ in 0..50 {
    let mut tail = list.split_off(500);
    list.append(&mut tail);
  }
  list.check_invariants();
  assert!(list.iter().copied().eq(0..1000));
  for k in 1000..3000 {
    list.push_back(k);
  }
  list.check_invariants();
  assert!(list.iter().copied().eq(0..3000));
}

#[test]
fn split_halves_outlive_each_other() {
  let mut list: SkipList<String> = (0..2000).map(|i| i.to_string()).collect();
  let tail = list.split_off(700);
  drop(list);
  assert_eq!(tail.len(), 1300);
  assert_eq!(tail[0], "700");
  let mut tail = tail;
  tail.push_back(String::from("end"));
  let mut head = tail.split_off(1);
  drop(tail);
  head.push_front(String::from("start"));
  head.check_invariants();
  assert_eq!(head.len(), 1301);
}

#[test]
#[should_panic]
fn insert_past_end_panics() {
  let mut list: SkipList<u64> = SkipList::new();
  list.insert(1, 0);
}

#[test]
#[should_panic]
fn remove_past_end_panics() {
  let mut list: SkipList<u64> = (0..3).collect();
  list.remove(3);
}

#[test]
fn traits_and_views() {
  let mut list: SkipList<u64> = (0..100).collect();
  for v in list.iter_mut() {
    *v *= 2;
  }
  assert_eq!(list[10], 20);
  list[10] = 7;
  assert_eq!(list.get(10), Some(&7));
  *list.front_mut().unwrap() = 1;
  *list.back_mut().unwrap() = 2;
  let clone = list.clone();
  clone.check_invariants();
  assert_eq!(clone, list);
  assert!(clone.clone().into_iter().eq(list.iter().copied()));
  assert!(clone.clone().into_iter().rev().eq(list.iter().rev().copied()));
  let oracle: VecDeque<u64> = list.iter().copied().collect();
  assert_eq!(format!("{list:?}"), format!("{oracle:?}"));
  let mut extended = SkipList::new();
  extended.extend([3u64, 4]);
  assert!(list < extended);
  assert_eq!(SkipList::<u64>::default().len(), 0);
  list.clear();
  assert!(list.is_empty());
  list.check_invariants();
}

#[test]
fn retain_panic_leaves_list_consistent() {
  let mut list: SkipList<u64> = (0..100).collect();
  let result = catch_unwind(AssertUnwindSafe(|| {
    list.retain(|v| {
      if *v == 60 {
        panic!("stop");
      }
      v % 2 == 0
    })
  }));
  assert!(result.is_err());
  list.check_invariants();
  let expected: Vec<u64> = (0..60).filter(|v| v % 2 == 0).chain(60..100).collect();
  assert!(list.iter().copied().eq(expected));
}

#[derive(Clone)]
struct Counted(Rc<Cell<usize>>);

impl Drop for Counted {
  fn drop(&mut self) {
    self.0.set(self.0.get() + 1);
  }
}

#[test]
fn every_value_dropped_once() {
  let drops = Rc::new(Cell::new(0));
  {
    let mut list: SkipList<Counted> = (0..100).map(|_| Counted(drops.clone())).collect();
    drop(list.remove(5));
    drop(list.pop_front());
    drop(list.pop_back());
    assert_eq!(drops.get(), 3);
    let tail = list.split_off(50);
    drop(tail);
    assert_eq!(drops.get(), 3 + 47);
    let mut it = list.clone().into_iter();
    drop(it.next());
    drop(it);
    assert_eq!(drops.get(), 3 + 47 + 50);
  }
  assert_eq!(drops.get(), 3 + 47 + 50 + 50);
}
