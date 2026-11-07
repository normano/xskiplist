//! Randomised and PCT schedules of the concurrent maps at a scale loom cannot reach. Node
//! capacity is 4 under `--cfg shuttle`, so a few dozen keys split and empty nodes constantly.
#![cfg(shuttle)]

use shuttle::rand::Rng;
use shuttle::sync::Arc;
use shuttle::thread;
use std::collections::BTreeMap;
use xskiplist::{ArenaSkipMap, ConcurrentSkipMap};

const ITERATIONS: usize = 2000;
const PCT_DEPTH: usize = 3;

fn disjoint_writers() {
  let threads = 4u64;
  let map = Arc::new(ConcurrentSkipMap::<u64, u64>::new());
  let handles: Vec<_> = (0..threads)
    .map(|t| {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        let mut rng = shuttle::rand::thread_rng();
        let mut oracle = BTreeMap::new();
        for _ in 0..40 {
          let k = rng.gen_range(0..12u64) * threads + t;
          if rng.gen_bool(0.35) {
            assert_eq!(map.remove(&k).map(|e| *e.value()), oracle.remove(&k));
          } else {
            let v: u64 = rng.r#gen();
            map.insert(k, v);
            oracle.insert(k, v);
          }
          assert_eq!(map.get(&k).map(|e| *e.value()), oracle.get(&k).copied());
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

fn contended_keys_with_readers() {
  let map = Arc::new(ConcurrentSkipMap::<u64, u64>::new());
  for k in 0..8 {
    map.insert(k, 0);
  }
  let writers: Vec<_> = (0..3u64)
    .map(|t| {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        let mut rng = shuttle::rand::thread_rng();
        for _ in 0..30 {
          let k = rng.gen_range(0..16u64);
          if rng.gen_bool(0.5) {
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
      for _ in 0..10 {
        let seen: Vec<u64> = map.iter().map(|e| *e.key()).collect();
        assert!(seen.windows(2).all(|w| w[0] < w[1]), "{seen:?}");
        let ranged: Vec<u64> = map.range(4..12).map(|e| *e.key()).collect();
        assert!(ranged.iter().all(|k| (4..12).contains(k)), "{ranged:?}");
        assert!(ranged.windows(2).all(|w| w[0] < w[1]));
      }
    })
  };
  for w in writers {
    w.join().unwrap();
  }
  reader.join().unwrap();
  map.check_invariants();
}

fn arena_inserts() {
  let map = Arc::new(ArenaSkipMap::<u64, u64>::new());
  let handles: Vec<_> = (0..4u64)
    .map(|t| {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        let mut rng = shuttle::rand::thread_rng();
        let mut wins = 0usize;
        for i in 0..20u64 {
          if map.insert(i * 4 + t, t).2 {
            wins += 1;
          }
          if map.insert(1000 + rng.gen_range(0..6u64), t).2 {
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
  for k in 0..80u64 {
    assert_eq!(map.get(&k), Some(&(k % 4)));
  }
}

#[test]
fn disjoint_writers_random() {
  shuttle::check_random(disjoint_writers, ITERATIONS);
}

#[test]
fn disjoint_writers_pct() {
  shuttle::check_pct(disjoint_writers, ITERATIONS, PCT_DEPTH);
}

#[test]
fn contended_keys_random() {
  shuttle::check_random(contended_keys_with_readers, ITERATIONS);
}

#[test]
fn contended_keys_pct() {
  shuttle::check_pct(contended_keys_with_readers, ITERATIONS, PCT_DEPTH);
}

#[test]
fn arena_inserts_random() {
  shuttle::check_random(arena_inserts, ITERATIONS);
}

#[test]
fn arena_inserts_pct() {
  shuttle::check_pct(arena_inserts, ITERATIONS, PCT_DEPTH);
}
