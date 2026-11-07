use super::ConcurrentSkipMap;
use crate::test_util::{scaled, scaled_mt};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;

struct Rng(u64);

impl Rng {
  fn next(&mut self) -> u64 {
    self.0 ^= self.0 << 13;
    self.0 ^= self.0 >> 7;
    self.0 ^= self.0 << 17;
    self.0
  }
}

fn assert_same(map: &ConcurrentSkipMap<u64, u64>, oracle: &BTreeMap<u64, u64>) {
  map.check_invariants();
  assert_eq!(map.len(), oracle.len());
  let ours: Vec<(u64, u64)> = map.iter().map(|e| (*e.key(), *e.value())).collect();
  let theirs: Vec<(u64, u64)> = oracle.iter().map(|(k, v)| (*k, *v)).collect();
  assert_eq!(ours, theirs);
  assert_eq!(map.front().map(|e| *e.key()), oracle.keys().next().copied());
  assert_eq!(map.back().map(|e| *e.key()), oracle.keys().next_back().copied());
}

#[test]
fn single_thread_matches_btreemap() {
  for seed in 1..=4u64 {
    for key_space in [8u64, 200, 5000] {
      let mut rng = Rng(seed);
      let map = ConcurrentSkipMap::new();
      let mut oracle = BTreeMap::new();
      for step in 0..scaled(6000u64) {
        let k = rng.next() % key_space;
        match rng.next() % 4 {
          0 | 1 => {
            let v = rng.next();
            assert_eq!(*map.insert(k, v).value(), v);
            oracle.insert(k, v);
          }
          2 => {
            let ours = map.remove(&k).map(|e| (*e.key(), *e.value()));
            let theirs = oracle.remove(&k).map(|v| (k, v));
            assert_eq!(ours, theirs, "remove {k}");
          }
          _ => {
            let v = rng.next();
            let got = *map.get_or_insert(k, v).value();
            let want = *oracle.entry(k).or_insert(v);
            assert_eq!(got, want);
          }
        }
        assert_eq!(map.get(&k).map(|e| *e.value()), oracle.get(&k).copied());
        if step % 500 == 0 {
          assert_same(&map, &oracle);
        }
      }
      assert_same(&map, &oracle);
      for _ in 0..scaled(100u64) {
        let a = rng.next() % (key_space + 10);
        let b = rng.next() % (key_space + 10);
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let ours: Vec<u64> = map.range(lo..hi).map(|e| *e.key()).collect();
        let theirs: Vec<u64> = oracle.range(lo..hi).map(|(k, _)| *k).collect();
        assert_eq!(ours, theirs, "range {lo}..{hi}");
        let ours: Vec<u64> = map.range(lo..=hi).map(|e| *e.key()).collect();
        let theirs: Vec<u64> = oracle.range(lo..=hi).map(|(k, _)| *k).collect();
        assert_eq!(ours, theirs, "range {lo}..={hi}");
        let ours: Vec<u64> = map
          .range((std::ops::Bound::Excluded(lo), std::ops::Bound::Unbounded))
          .map(|e| *e.key())
          .collect();
        let theirs: Vec<u64> = oracle
          .range((std::ops::Bound::Excluded(lo), std::ops::Bound::Unbounded))
          .map(|(k, _)| *k)
          .collect();
        assert_eq!(ours, theirs);
      }
    }
  }
}

#[test]
fn drain_and_refill() {
  let map = ConcurrentSkipMap::new();
  let n = scaled(3000u64);
  for round in 0..3 {
    for i in 0..n {
      map.insert(i * 3, round);
    }
    map.check_invariants();
    assert_eq!(map.len(), n as usize);
    for i in (0..n).step_by(2) {
      assert!(map.remove(&(i * 3)).is_some());
    }
    map.check_invariants();
    for i in (1..n).step_by(2) {
      assert!(map.remove(&(i * 3)).is_some());
    }
    assert!(map.is_empty());
    assert!(map.front().is_none());
    assert!(map.iter().next().is_none());
    map.check_invariants();
  }
}

#[test]
fn disjoint_writers_match_their_oracles() {
  let threads = 8u64;
  let map = Arc::new(ConcurrentSkipMap::<u64, u64>::new());
  let handles: Vec<_> = (0..threads)
    .map(|t| {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        let mut rng = Rng(t * 7 + 1);
        let mut oracle = BTreeMap::new();
        for _ in 0..scaled_mt(30_000u64) {
          let k = (rng.next() % 4000) * threads + t;
          if rng.next().is_multiple_of(3) {
            let ours = map.remove(&k).map(|e| *e.value());
            assert_eq!(ours, oracle.remove(&k));
          } else {
            let v = rng.next();
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
  assert_same(&map, &union);
}

#[test]
fn contended_keys_stay_consistent() {
  let threads = 8u64;
  let map = Arc::new(ConcurrentSkipMap::<u64, u64>::new());
  let stop = Arc::new(AtomicBool::new(false));
  let readers: Vec<_> = (0..2)
    .map(|_| {
      let map = Arc::clone(&map);
      let stop = Arc::clone(&stop);
      thread::spawn(move || {
        let mut scans = 0u64;
        while !stop.load(Ordering::Relaxed) {
          let mut prev = None;
          for e in map.iter() {
            if let Some(p) = prev {
              assert!(p < *e.key(), "iteration out of order");
            }
            prev = Some(*e.key());
          }
          for e in map.range(100..200) {
            assert!((100..200).contains(e.key()));
          }
          scans += 1;
        }
        scans
      })
    })
    .collect();
  let writers: Vec<_> = (0..threads)
    .map(|t| {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        let mut rng = Rng(t + 11);
        for _ in 0..scaled_mt(20_000u64) {
          let k = rng.next() % 300;
          if rng.next().is_multiple_of(2) {
            map.insert(k, t);
          } else {
            map.remove(&k);
          }
        }
      })
    })
    .collect();
  for w in writers {
    w.join().unwrap();
  }
  stop.store(true, Ordering::Relaxed);
  for r in readers {
    assert!(r.join().unwrap() > 0);
  }
  map.check_invariants();
  let keys: Vec<u64> = map.iter().map(|e| *e.key()).collect();
  assert!(keys.windows(2).all(|w| w[0] < w[1]));
  assert!(keys.iter().all(|k| *k < 300));
  assert_eq!(keys.len(), map.len());
}

#[test]
fn entries_outlive_removal() {
  let map = ConcurrentSkipMap::new();
  map.insert(1, String::from("one"));
  let held = map.get(&1).unwrap();
  let removed = map.remove(&1).unwrap();
  assert!(map.get(&1).is_none());
  assert_eq!(held.value(), "one");
  assert_eq!(removed.value(), "one");
  map.insert(1, String::from("uno"));
  assert_eq!(held.value(), "one");
  assert_eq!(map.get(&1).unwrap().value(), "uno");
}

#[test]
fn every_value_dropped_once() {
  struct Counted(Arc<AtomicUsize>);
  impl Drop for Counted {
    fn drop(&mut self) {
      self.0.fetch_add(1, Ordering::Relaxed);
    }
  }
  let drops = Arc::new(AtomicUsize::new(0));
  let n = scaled(4000usize);
  let keys = n.min(700);
  let removed = keys.div_ceil(3);
  {
    let map = ConcurrentSkipMap::new();
    for i in 0..n {
      map.insert(i % keys, Counted(Arc::clone(&drops)));
    }
    for i in (0..keys).step_by(3) {
      map.remove(&i);
    }
    assert_eq!(map.len(), keys - removed);
  }
  assert!(
    drops.load(Ordering::Relaxed) >= keys - removed,
    "live entries dropped with the map"
  );
  assert!(drops.load(Ordering::Relaxed) <= n);
  let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
  while drops.load(Ordering::Relaxed) != n && std::time::Instant::now() < deadline {
    crossbeam_epoch::pin().flush();
    thread::yield_now();
  }
  assert_eq!(
    drops.load(Ordering::Relaxed),
    n,
    "removed entries reclaimed once every pin is released"
  );
}

#[test]
fn string_keys_borrow() {
  let map: ConcurrentSkipMap<String, usize> = [("b".to_string(), 2), ("a".to_string(), 1)].into_iter().collect();
  assert_eq!(map.get("a").map(|e| *e.value()), Some(1));
  assert!(map.get("c").is_none());
  assert!(map.remove("b").is_some());
  let keys: Vec<String> = map.iter().map(|e| e.key().clone()).collect();
  assert_eq!(keys, ["a"]);
}
