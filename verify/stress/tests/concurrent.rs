//! Real-hardware stress gates for the concurrent maps. Release build only: in a debug build this
//! file compiles to nothing, so run it through `verify/scripts/stress.sh`. nextest applies the
//! timeout in `.config/nextest.toml`, so a hang fails instead of sitting.
#![cfg(not(debug_assertions))]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};
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

fn cores() -> u64 {
  thread::available_parallelism().map(|n| n.get() as u64).unwrap_or(8)
}

#[test]
fn disjoint_writers_at_scale() {
  let threads = cores();
  let map = Arc::new(ConcurrentSkipMap::<u64, u64>::new());
  let handles: Vec<_> = (0..threads)
    .map(|t| {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        let mut rng = Rng(t * 977 + 1);
        let mut oracle = BTreeMap::new();
        for _ in 0..2_000_000u64 {
          let k = (rng.next() % 50_000) * threads + t;
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
  assert_eq!(map.len(), union.len());
  let ours: Vec<(u64, u64)> = map.iter().map(|e| (*e.key(), *e.value())).collect();
  let theirs: Vec<(u64, u64)> = union.into_iter().collect();
  assert_eq!(ours, theirs);
}

#[test]
fn contended_keys_with_readers_for_twenty_seconds() {
  let map = Arc::new(ConcurrentSkipMap::<u64, u64>::new());
  let stop = Arc::new(AtomicBool::new(false));
  let scans = Arc::new(AtomicUsize::new(0));
  let readers: Vec<_> = (0..2)
    .map(|_| {
      let map = Arc::clone(&map);
      let stop = Arc::clone(&stop);
      let scans = Arc::clone(&scans);
      thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
          let mut prev = None;
          for e in map.iter() {
            if let Some(p) = prev {
              assert!(p < *e.key(), "iteration out of order");
            }
            prev = Some(*e.key());
          }
          for e in map.range(1000..2000) {
            assert!((1000..2000).contains(e.key()));
          }
          scans.fetch_add(1, Ordering::Relaxed);
        }
      })
    })
    .collect();
  let deadline = Instant::now() + Duration::from_secs(20);
  let writers: Vec<_> = (0..cores() - 2)
    .map(|t| {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        let mut rng = Rng(t + 11);
        while Instant::now() < deadline {
          for _ in 0..4096 {
            let k = rng.next() % 3000;
            if rng.next().is_multiple_of(2) {
              map.insert(k, t);
            } else {
              map.remove(&k);
            }
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
    r.join().unwrap();
  }
  assert!(scans.load(Ordering::Relaxed) > 0);
  map.check_invariants();
  let keys: Vec<u64> = map.iter().map(|e| *e.key()).collect();
  assert!(keys.windows(2).all(|w| w[0] < w[1]));
  assert_eq!(keys.len(), map.len());
}

#[test]
fn every_entry_reclaimed_at_scale() {
  struct Counted(Arc<AtomicUsize>);
  impl Drop for Counted {
    fn drop(&mut self) {
      self.0.fetch_add(1, Ordering::Relaxed);
    }
  }
  let drops = Arc::new(AtomicUsize::new(0));
  let created = Arc::new(AtomicUsize::new(0));
  let threads = cores();
  let per = 200_000usize;
  {
    let map = Arc::new(ConcurrentSkipMap::<u64, Counted>::new());
    let handles: Vec<_> = (0..threads)
      .map(|t| {
        let map = Arc::clone(&map);
        let drops = Arc::clone(&drops);
        let created = Arc::clone(&created);
        thread::spawn(move || {
          let mut rng = Rng(t + 3);
          for _ in 0..per {
            let k = rng.next() % 20_000;
            if rng.next().is_multiple_of(2) {
              created.fetch_add(1, Ordering::Relaxed);
              map.insert(k, Counted(Arc::clone(&drops)));
            } else {
              map.remove(&k);
            }
          }
        })
      })
      .collect();
    for h in handles {
      h.join().unwrap();
    }
    map.check_invariants();
  }
  let created = created.load(Ordering::Relaxed);
  let deadline = Instant::now() + Duration::from_secs(30);
  while drops.load(Ordering::Relaxed) != created && Instant::now() < deadline {
    crossbeam_epoch::pin().flush();
    thread::yield_now();
  }
  assert_eq!(
    drops.load(Ordering::Relaxed),
    created,
    "every value dropped exactly once"
  );
}

#[test]
fn arena_fill_from_every_core() {
  let threads = cores();
  let per = 1_000_000u64;
  let map = Arc::new(ArenaSkipMap::<u64, u64>::new());
  let handles: Vec<_> = (0..threads)
    .map(|t| {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        for i in 0..per {
          assert!(map.insert(i * threads + t, t).2);
        }
      })
    })
    .collect();
  for h in handles {
    h.join().unwrap();
  }
  map.check_invariants();
  assert_eq!(map.len(), (per * threads) as usize);
  let mut rng = Rng(5);
  for _ in 0..100_000 {
    let k = rng.next() % (per * threads);
    assert_eq!(map.get(&k), Some(&(k % threads)));
  }
  let mut prev = None;
  for (k, _) in map.iter() {
    if let Some(p) = prev {
      assert!(p < *k);
    }
    prev = Some(*k);
  }
}
