//! Exhaustive models of the ConcurrentSkipMap protocols. Node capacity is 2 under `--cfg loom`,
//! so three keys split a node and removing both keys of a node unlinks it.
#![cfg(loom)]

use loom::sync::Arc;
use loom::thread;
use xskiplist::ConcurrentSkipMap;

fn keys(map: &ConcurrentSkipMap<u64, u64>) -> Vec<u64> {
  map.iter().map(|e| *e.key()).collect()
}

#[test]
fn split_under_a_reader() {
  loom::model(|| {
    let map = Arc::new(ConcurrentSkipMap::new());
    map.insert(1, 1);
    map.insert(2, 2);
    let writer = {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        map.insert(3, 3);
      })
    };
    let reader = {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        assert_eq!(map.get(&2).map(|e| *e.value()), Some(2));
        let seen = keys(&map);
        assert!(seen.windows(2).all(|w| w[0] < w[1]), "iteration out of order: {seen:?}");
        assert!(
          seen.contains(&1) && seen.contains(&2),
          "entries lost during split: {seen:?}"
        );
      })
    };
    writer.join().unwrap();
    reader.join().unwrap();
    map.check_invariants();
    assert_eq!(keys(&map), [1, 2, 3]);
  });
}

#[test]
fn two_splits_of_one_node() {
  loom::model(|| {
    let map = Arc::new(ConcurrentSkipMap::new());
    map.insert(10, 0);
    map.insert(20, 0);
    let a = {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        map.insert(15, 0);
      })
    };
    let b = {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        map.insert(25, 0);
      })
    };
    a.join().unwrap();
    b.join().unwrap();
    map.check_invariants();
    assert_eq!(keys(&map), [10, 15, 20, 25]);
  });
}

#[test]
fn unlink_under_an_insert() {
  loom::model(|| {
    let map = Arc::new(ConcurrentSkipMap::new());
    map.insert(5, 5);
    let remover = {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        assert_eq!(map.remove(&5).map(|e| *e.value()), Some(5));
      })
    };
    let inserter = {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        map.insert(6, 6);
      })
    };
    remover.join().unwrap();
    inserter.join().unwrap();
    map.check_invariants();
    assert_eq!(keys(&map), [6]);
    assert_eq!(map.get(&6).map(|e| *e.value()), Some(6));
  });
}

#[test]
fn unlink_under_a_reader() {
  loom::model(|| {
    let map = Arc::new(ConcurrentSkipMap::new());
    map.insert(1, 1);
    map.insert(2, 2);
    map.insert(3, 3);
    let remover = {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        map.remove(&3);
        map.remove(&2);
      })
    };
    let reader = {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        assert_eq!(map.get(&1).map(|e| *e.value()), Some(1));
        let seen = keys(&map);
        assert!(seen.windows(2).all(|w| w[0] < w[1]));
        assert!(seen.contains(&1));
      })
    };
    remover.join().unwrap();
    reader.join().unwrap();
    map.check_invariants();
    assert_eq!(keys(&map), [1]);
  });
}

#[test]
fn same_key_from_two_threads() {
  loom::model(|| {
    let map = Arc::new(ConcurrentSkipMap::new());
    let a = {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        map.insert(1, 10);
      })
    };
    let b = {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        map.insert(1, 20);
      })
    };
    a.join().unwrap();
    b.join().unwrap();
    map.check_invariants();
    let v = map.get(&1).map(|e| *e.value());
    assert!(v == Some(10) || v == Some(20), "{v:?}");
    assert_eq!(map.len(), 1);
  });
}

#[test]
fn remove_against_insert_of_the_same_key() {
  loom::model(|| {
    let map = Arc::new(ConcurrentSkipMap::new());
    map.insert(1, 1);
    let remover = {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        map.remove(&1);
      })
    };
    let inserter = {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        map.insert(1, 2);
      })
    };
    remover.join().unwrap();
    inserter.join().unwrap();
    map.check_invariants();
    let v = map.get(&1).map(|e| *e.value());
    assert!(v.is_none() || v == Some(2), "{v:?}");
    assert_eq!(map.len(), usize::from(v.is_some()));
  });
}

#[test]
fn range_start_under_a_split() {
  loom::model(|| {
    let map = Arc::new(ConcurrentSkipMap::new());
    map.insert(10, 0);
    map.insert(20, 0);
    let writer = {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        map.insert(30, 0);
      })
    };
    let reader = {
      let map = Arc::clone(&map);
      thread::spawn(move || {
        let seen: Vec<u64> = map.range(25..).map(|e| *e.key()).collect();
        assert!(
          seen.iter().all(|k| *k >= 25),
          "range yielded a key below its start: {seen:?}"
        );
        let seen: Vec<u64> = map.range(15..25).map(|e| *e.key()).collect();
        assert!(seen.iter().all(|k| (15..25).contains(k)), "{seen:?}");
        assert!(seen.contains(&20), "entry lost during split: {seen:?}");
      })
    };
    writer.join().unwrap();
    reader.join().unwrap();
    map.check_invariants();
    assert_eq!(keys(&map), [10, 20, 30]);
  });
}
