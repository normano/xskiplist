//! Exhaustive models of the ArenaSkipMap insert protocol. Arena chunks are 1 KiB under
//! `--cfg loom`, so a few nodes refill a stripe.
#![cfg(loom)]

use loom::sync::Arc;
use loom::thread;
use xskiplist::ArenaSkipMap;

fn keys(map: &ArenaSkipMap<u64, u64>) -> Vec<u64> {
  map.iter().map(|(k, _)| *k).collect()
}

#[test]
fn same_key_lands_once() {
  loom::model(|| {
    let map = Arc::new(ArenaSkipMap::new());
    let a = {
      let map = Arc::clone(&map);
      thread::spawn(move || map.insert(1, 10).2)
    };
    let b = {
      let map = Arc::clone(&map);
      thread::spawn(move || map.insert(1, 20).2)
    };
    let won_a = a.join().unwrap();
    let won_b = b.join().unwrap();
    assert!(won_a != won_b, "exactly one insert of a key wins");
    map.check_invariants();
    assert_eq!(map.len(), 1);
    let v = *map.get(&1).unwrap();
    assert_eq!(v, if won_a { 10 } else { 20 });
  });
}

#[test]
fn adjacent_keys_from_two_threads() {
  loom::model(|| {
    let map = Arc::new(ArenaSkipMap::new());
    map.insert(5, 0);
    let a = {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        map.insert(3, 0);
        map.insert(7, 0);
      })
    };
    let b = {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        map.insert(4, 0);
        map.insert(6, 0);
      })
    };
    a.join().unwrap();
    b.join().unwrap();
    map.check_invariants();
    assert_eq!(keys(&map), [3, 4, 5, 6, 7]);
  });
}

#[test]
fn reader_sees_a_sorted_prefix() {
  loom::model(|| {
    let map = Arc::new(ArenaSkipMap::new());
    map.insert(2, 2);
    let writer = {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        map.insert(1, 1);
        map.insert(3, 3);
      })
    };
    let reader = {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        let seen = keys(&map);
        assert!(seen.windows(2).all(|w| w[0] < w[1]), "{seen:?}");
        assert!(seen.contains(&2));
        assert_eq!(map.get(&2), Some(&2));
      })
    };
    writer.join().unwrap();
    reader.join().unwrap();
    map.check_invariants();
    assert_eq!(keys(&map), [1, 2, 3]);
  });
}
