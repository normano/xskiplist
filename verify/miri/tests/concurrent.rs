//! Threaded scenarios sized for Miri's race detector: three threads and a few dozen operations,
//! so one seed runs in seconds and `verify/scripts/miri.sh` can sweep many seeds. They also run
//! natively as ordinary tests.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::thread;
use xskiplist::{ArenaSkipMap, ConcurrentSkipMap};

struct Rng(u64);

impl Rng {
  fn next(&mut self) -> u64 {
    self.0 ^= self.0 << 13;
    self.0 ^= self.0 >> 7;
    self.0 ^= self.0 << 17;
    self.0
  }
}

#[test]
fn concurrent_disjoint_writers() {
  let threads = 3u64;
  let map = Arc::new(ConcurrentSkipMap::<u64, u64>::new());
  let handles: Vec<_> = (0..threads)
    .map(|t| {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        let mut rng = Rng(t * 31 + 7);
        let mut oracle = BTreeMap::new();
        for _ in 0..40 {
          let k = (rng.next() % 24) * threads + t;
          if rng.next().is_multiple_of(3) {
            assert_eq!(map.remove(&k).map(|e| *e.value()), oracle.remove(&k));
          } else {
            let v = rng.next();
            map.insert(k, v);
            oracle.insert(k, v);
          }
        }
        oracle
      })
    })
    .collect();
  let mut union = BTreeMap::new();
  for h in handles {
    union.extend(h.join().unwrap());
  }
  map.check_invariants();
  let ours: Vec<(u64, u64)> = map.iter().map(|e| (*e.key(), *e.value())).collect();
  let theirs: Vec<(u64, u64)> = union.into_iter().collect();
  assert_eq!(ours, theirs);
}

#[test]
fn concurrent_contended_keys_with_a_reader() {
  let map = Arc::new(ConcurrentSkipMap::<u64, u64>::new());
  for k in 0..40 {
    map.insert(k, 0);
  }
  let writers: Vec<_> = (0..2u64)
    .map(|t| {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        let mut rng = Rng(t + 1);
        for _ in 0..60 {
          let k = rng.next() % 80;
          if rng.next().is_multiple_of(2) {
            map.insert(k, t);
          } else {
            map.remove(&k);
          }
        }
      })
    })
    .collect();
  let reader = {
    let map = Arc::clone(&map);
    thread::spawn(move || {
      for _ in 0..6 {
        let seen: Vec<u64> = map.iter().map(|e| *e.key()).collect();
        assert!(seen.windows(2).all(|w| w[0] < w[1]), "{seen:?}");
        for e in map.range(20..60) {
          assert!((20..60).contains(e.key()));
        }
      }
    })
  };
  for w in writers {
    w.join().unwrap();
  }
  reader.join().unwrap();
  map.check_invariants();
}

#[test]
fn arena_racing_inserts() {
  let map = Arc::new(ArenaSkipMap::<u64, u64>::new());
  let handles: Vec<_> = (0..3u64)
    .map(|t| {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        let mut wins = 0usize;
        for i in 0..30u64 {
          if map.insert(i * 3 + t, t).2 {
            wins += 1;
          }
          if map.insert(500 + (i % 5), t).2 {
            wins += 1;
          }
        }
        wins
      })
    })
    .collect();
  let wins: usize = handles.into_iter().map(|h| h.join().unwrap()).sum();
  map.check_invariants();
  assert_eq!(map.len(), wins);
  for k in 0..90u64 {
    assert_eq!(map.get(&k), Some(&(k % 3)));
  }
}
