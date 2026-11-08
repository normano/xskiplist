//! Fixed-duration cells for concurrent sorted maps over a thread-count axis.
//!
//! Every cell prebuilds a map of `n` even keys, starts `threads` workers and runs them for
//! `--limit` seconds. Mixed cells pick a key from `0..2n` per step and read it or, with the
//! cell's write share, insert it or remove it. `range` cells scan 100 entries from a random key
//! while one thread writes. `fill` cells insert distinct fresh keys into an empty map, starting a
//! new map after every million. Steps are summed over the workers and ns/step is per worker, so
//! two workers each taking 50 ns report 50.
//!
//! `cargo bench --bench concurrent -- [--limit S] [--impls a,b] [--threads 1,2,4] [--sizes n] [--only substr] [--skip substr]`

use std::collections::BTreeMap;
use std::hint::black_box;
use std::sync::Barrier;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const SCAN: usize = 100;
const FILL_ROUND: u64 = 1 << 20;

struct Rng(u64);

impl Rng {
  fn next(&mut self) -> u64 {
    self.0 ^= self.0 << 13;
    self.0 ^= self.0 >> 7;
    self.0 ^= self.0 << 17;
    self.0
  }
}

// MARK: Map API

trait ConcMap: Sync + Send + 'static {
  /// Whether the map removes keys. Cells that remove are skipped otherwise.
  const REMOVES: bool = true;
  fn new() -> Self;
  fn insert(&self, key: u64, value: u64);
  fn get(&self, key: u64) -> Option<u64>;
  fn remove(&self, key: u64) -> bool;
  fn range_sum(&self, lo: u64, count: usize) -> u64;
}

impl ConcMap for xskiplist::ArenaSkipMap<u64, u64> {
  const REMOVES: bool = false;
  fn new() -> Self {
    xskiplist::ArenaSkipMap::new()
  }
  fn insert(&self, key: u64, value: u64) {
    self.insert(key, value);
  }
  fn get(&self, key: u64) -> Option<u64> {
    self.get(&key).copied()
  }
  fn remove(&self, _key: u64) -> bool {
    unreachable!("ArenaSkipMap cannot remove")
  }
  fn range_sum(&self, lo: u64, count: usize) -> u64 {
    self.range(lo..).take(count).map(|(_, v)| *v).sum()
  }
}

impl ConcMap for xskiplist::ConcurrentSkipMap<u64, u64> {
  fn new() -> Self {
    xskiplist::ConcurrentSkipMap::new()
  }
  fn insert(&self, key: u64, value: u64) {
    self.insert(key, value);
  }
  fn get(&self, key: u64) -> Option<u64> {
    self.get(&key).map(|e| *e.value())
  }
  fn remove(&self, key: u64) -> bool {
    self.remove(&key).is_some()
  }
  fn range_sum(&self, lo: u64, count: usize) -> u64 {
    self.range(lo..).take(count).map(|e| *e.value()).sum()
  }
}

impl ConcMap for crossbeam_skiplist::SkipMap<u64, u64> {
  fn new() -> Self {
    crossbeam_skiplist::SkipMap::new()
  }
  fn insert(&self, key: u64, value: u64) {
    self.insert(key, value);
  }
  fn get(&self, key: u64) -> Option<u64> {
    self.get(&key).map(|e| *e.value())
  }
  fn remove(&self, key: u64) -> bool {
    self.remove(&key).is_some()
  }
  fn range_sum(&self, lo: u64, count: usize) -> u64 {
    self.range(lo..).take(count).map(|e| *e.value()).sum()
  }
}

struct RwBTree(parking_lot::RwLock<BTreeMap<u64, u64>>);

impl ConcMap for RwBTree {
  fn new() -> Self {
    RwBTree(parking_lot::RwLock::new(BTreeMap::new()))
  }
  fn insert(&self, key: u64, value: u64) {
    self.0.write().insert(key, value);
  }
  fn get(&self, key: u64) -> Option<u64> {
    self.0.read().get(&key).copied()
  }
  fn remove(&self, key: u64) -> bool {
    self.0.write().remove(&key).is_some()
  }
  fn range_sum(&self, lo: u64, count: usize) -> u64 {
    self.0.read().range(lo..).take(count).map(|(_, v)| *v).sum()
  }
}

struct MutexSkip(parking_lot::Mutex<xskiplist::SkipMap<u64, u64>>);

impl ConcMap for MutexSkip {
  fn new() -> Self {
    MutexSkip(parking_lot::Mutex::new(xskiplist::SkipMap::new()))
  }
  fn insert(&self, key: u64, value: u64) {
    self.0.lock().insert(key, value);
  }
  fn get(&self, key: u64) -> Option<u64> {
    self.0.lock().get(&key).copied()
  }
  fn remove(&self, key: u64) -> bool {
    self.0.lock().remove(&key).is_some()
  }
  fn range_sum(&self, lo: u64, count: usize) -> u64 {
    self.0.lock().range(lo..).take(count).map(|(_, v)| *v).sum()
  }
}

// MARK: Cells

struct Cell {
  label: &'static str,
  run: Box<dyn Fn(usize, usize, Duration) -> u64>,
}

fn prebuilt<M: ConcMap>(n: usize) -> M {
  let m = M::new();
  for i in 0..n as u64 {
    m.insert(2 * i, i);
  }
  m
}

/// Runs `step` on `threads` workers until `limit` has passed and returns the total step count.
/// `step` gets the worker's index, its RNG and a step counter.
fn run_workers<M: ConcMap>(
  map: &M,
  threads: usize,
  limit: Duration,
  step: impl Fn(&M, usize, &mut Rng, u64) + Sync,
) -> u64 {
  let stop = AtomicBool::new(false);
  let barrier = Barrier::new(threads + 1);
  let totals: Vec<u64> = thread::scope(|s| {
    let handles: Vec<_> = (0..threads)
      .map(|t| {
        let step = &step;
        let stop = &stop;
        let barrier = &barrier;
        s.spawn(move || {
          let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ ((t as u64 + 1) << 32));
          barrier.wait();
          let mut count = 0u64;
          while !stop.load(Ordering::Relaxed) {
            for _ in 0..256 {
              step(map, t, &mut rng, count);
              count += 1;
            }
          }
          count
        })
      })
      .collect();
    barrier.wait();
    thread::sleep(limit);
    stop.store(true, Ordering::Relaxed);
    handles.into_iter().map(|h| h.join().unwrap()).collect()
  });
  totals.iter().sum()
}

fn mixed<M: ConcMap>(label: &'static str, write_share: u64) -> Cell {
  Cell {
    label,
    run: Box::new(move |n, threads, limit| {
      let map: M = prebuilt(n);
      let span = 2 * n as u64;
      run_workers(&map, threads, limit, |m, _, rng, _| {
        let r = rng.next();
        let key = (r >> 8) % span;
        if (r & 0xff) < write_share {
          if r & 0x100 == 0 {
            m.insert(key, key);
          } else {
            black_box(m.remove(key));
          }
        } else {
          black_box(m.get(key));
        }
      })
    }),
  }
}

fn range<M: ConcMap>() -> Cell {
  Cell {
    label: "range",
    run: Box::new(|n, threads, limit| {
      let map: M = prebuilt(n);
      let span = 2 * n as u64;
      run_workers(&map, threads, limit, |m, t, rng, _| {
        let r = rng.next();
        let key = (r >> 8) % span;
        if t == 0 && threads > 1 {
          if !M::REMOVES || r & 0x100 == 0 {
            m.insert(key, key);
          } else {
            black_box(m.remove(key));
          }
        } else {
          black_box(m.range_sum(key, SCAN));
        }
      })
    }),
  }
}

fn fill<M: ConcMap>() -> Cell {
  Cell {
    label: "fill",
    run: Box::new(|_, threads, limit| {
      let stop = AtomicBool::new(false);
      let total = std::sync::atomic::AtomicU64::new(0);
      thread::scope(|s| {
        s.spawn(|| {
          thread::sleep(limit);
          stop.store(true, Ordering::Relaxed);
        });
        while !stop.load(Ordering::Relaxed) {
          let map = M::new();
          let per = FILL_ROUND / threads as u64;
          let round: u64 = thread::scope(|round| {
            let handles: Vec<_> = (0..threads as u64)
              .map(|t| {
                let map = &map;
                let stop = &stop;
                round.spawn(move || {
                  let mut rng = Rng(0x2545_F491_4F6C_DD1D ^ ((t + 1) << 32));
                  let mut done = 0u64;
                  for i in 0..per {
                    if i & 0xfff == 0 && stop.load(Ordering::Relaxed) {
                      break;
                    }
                    let key = (rng.next() << 8) | (t << 4) | 1;
                    map.insert(key ^ (i << 12), i);
                    done += 1;
                  }
                  done
                })
              })
              .collect();
            handles.into_iter().map(|h| h.join().unwrap()).sum()
          });
          total.fetch_add(round, Ordering::Relaxed);
          black_box(&map);
        }
      });
      total.load(Ordering::Relaxed)
    }),
  }
}

fn cells<M: ConcMap>() -> Vec<Cell> {
  if M::REMOVES {
    vec![
      mixed::<M>("read", 0),
      mixed::<M>("read90_write10", 26),
      mixed::<M>("read50_write50", 128),
      mixed::<M>("write", 256),
      range::<M>(),
      fill::<M>(),
    ]
  } else {
    vec![mixed::<M>("read", 0), range::<M>(), fill::<M>()]
  }
}

// MARK: Driver

fn list_arg(args: &[String], name: &str, default: &[&str]) -> Vec<String> {
  match args.iter().position(|a| a == name) {
    Some(i) => args[i + 1].split(',').map(str::to_string).collect(),
    None => default.iter().map(|s| s.to_string()).collect(),
  }
}

fn main() {
  let args: Vec<String> = std::env::args().collect();
  let limit = Duration::from_secs_f64(list_arg(&args, "--limit", &["10"])[0].parse().expect("--limit seconds"));
  let impls = list_arg(&args, "--impls", &["crossbeam", "rwlock_btree", "mutex_skipmap"]);
  let threads: Vec<usize> = list_arg(&args, "--threads", &["1", "2", "4", "8", "14"])
    .iter()
    .map(|s| s.parse().expect("--threads"))
    .collect();
  let sizes: Vec<usize> = list_arg(&args, "--sizes", &["100000"])
    .iter()
    .map(|s| s.parse().expect("--sizes"))
    .collect();
  let only = args.iter().position(|a| a == "--only").map(|i| args[i + 1].clone());
  let skip = args.iter().position(|a| a == "--skip").map(|i| args[i + 1].clone());
  println!(
    "{:<14}{:<18}{:>4}{:>9}{:>14}{:>10}",
    "impl", "workload", "thr", "n", "steps", "ns/step"
  );
  for name in &impls {
    let cells = match name.as_str() {
      "arena" => cells::<xskiplist::ArenaSkipMap<u64, u64>>(),
      "concurrent" => cells::<xskiplist::ConcurrentSkipMap<u64, u64>>(),
      "crossbeam" => cells::<crossbeam_skiplist::SkipMap<u64, u64>>(),
      "rwlock_btree" => cells::<RwBTree>(),
      "mutex_skipmap" => cells::<MutexSkip>(),
      other => panic!("unknown impl {other}"),
    };
    for cell in &cells {
      if let Some(o) = &only
        && !cell.label.contains(o.as_str())
      {
        continue;
      }
      if let Some(k) = &skip
        && cell.label.contains(k.as_str())
      {
        continue;
      }
      for &n in &sizes {
        for &t in &threads {
          let start = Instant::now();
          let steps = (cell.run)(n, t, limit);
          let elapsed = start.elapsed();
          let ns = elapsed.as_nanos() as f64 * t as f64 / steps.max(1) as f64;
          println!(
            "{:<14}{:<18}{:>4}{:>9}{:>14}{:>10.1}",
            name, cell.label, t, n, steps, ns
          );
        }
      }
    }
  }
}
