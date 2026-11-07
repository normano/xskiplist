# Memory-model readings

Pairings whose correctness rests on something no checker in `verify/` models. Each entry names the two sides, what the argument rests on and its status. Rust atomics follow the C++20 memory model.

## Consume-ordered tower loads

Sides:

- **Publisher**: writes a node's key, value and tower (or the concurrent map's anchor, flags and slots), then publishes the node with a release store or release CAS into a predecessor's tower.
- **Reader**: loads the predecessor's tower entry with `Ordering::Relaxed` followed by `compiler_fence(Acquire)` (`next()` in `arena.rs` and `concurrent.rs`), then reads the node's fields through the loaded pointer.

Required: the reader sees the fields the publisher wrote before publishing.

Reading: in C++20 terms the relaxed load does not synchronise with the release store, so the field reads are a data race and the outcome is undefined. On every target this crate supports the reads are address-dependent on the loaded pointer. No shipping AArch64 or x86 implementation reorders a load before the load that produced its address (AArch64 guarantees it architecturally; x86 TSO orders all loads). The compiler fence stops the compiler from hoisting or speculating the dependent reads. This is the `load_consume` that crossbeam-epoch and crossbeam-skiplist ship on the same reasoning; `memory_order_consume` exists in C++ for exactly this pattern and no compiler implements it as anything but acquire.

Checking: none of the tiers can judge it. Miri, loom, shuttle and TSan all get an acquire load instead (`cfg!(miri)`, `sync::MODELED`, `cfg!(xskiplist_tsan)`). What they verify is that the protocol is correct given acquire; what they cannot verify is that consume suffices on the hardware. Measured cost of acquire instead of consume: 2% to 6% on reads (concurrent ledger, prelim 3).

Status: accepted, matching crossbeam. A target without dependency ordering would need `MODELED`-style acquire loads.

## Seqlock snapshots of a node's slots

Sides:

- **Writer** (`begin_write`, slot stores, `len` store, `end_write`), holding the node's lock: stores `seq | 1` relaxed, `fence(Release)`, writes the slots and `len` relaxed, stores `seq + 1` with release.
- **Reader** (`Snapshot::take`): loads `seq` with acquire, retries while odd, copies `len` and the slots with relaxed loads, `fence(Acquire)`, loads `seq` relaxed and retries unless equal.

Required: a snapshot that validates was not torn by a writer.

Reading: this is the crossbeam-utils `SeqLock` protocol. If the writer's odd store is visible to the reader's first load, the reader retries. If it is not, every slot write is sequenced after `fence(Release)`, which is after the odd store, so a reader whose acquire fence follows its slot reads and whose second load still sees the even value read slots that no writer had started overwriting. The slot and `len` reads are relaxed atomic loads, not plain reads, so there is no data race even when the snapshot is torn and discarded. The entries a validated snapshot points to were in the node after the reader pinned, so the epoch keeps them allocated.

Checking: loom (`split_under_a_reader`, `unlink_under_a_reader`) explores the pairing exhaustively with capacity 2; Miri's race detector runs it under sampled schedules; TSan under real ones.

Status: verified by the tiers above.

## Split publication order

Sides:

- **Splitter**, holding the node's lock and its predecessors' locks above the node's height: fills the new node's slots and tower, then stores it into the predecessors' towers level 0 first, each with release, then sets `linked` with release.
- **Reader**: descends through tower loads (acquire under the checkers, consume in production), reads the new node's anchor and snapshots its slots.

Required: a reader that reaches the new node sees a complete node. A node is never reachable at a level above one where it is not yet linked.

Reading: the release store at level 0 happens after every write to the new node, so the first pointer any reader can follow publishes the whole node. Higher levels are stored after level 0, so a reader arriving through a higher level finds the lower levels already linked. The remover waits on `linked` with acquire before touching the node's tower, so an unlink never races a half-linked tower.

Checking: loom (`split_under_a_reader`, `two_splits_of_one_node`, `unlink_under_an_insert`), shuttle at capacity 4, Miri and TSan.

Status: verified by the tiers above.
