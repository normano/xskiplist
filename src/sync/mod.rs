//! The primitives the concurrent maps are built on, selected once for the whole crate. Normal
//! builds re-export std and parking_lot. `--cfg loom` swaps in loom's modelled types and
//! `--cfg shuttle` shuttle's, so the model checkers in `verify/` see every atomic step. This is
//! the only `cfg(loom)` / `cfg(shuttle)` site in the tree.
//!
//! Anything that blocks or spins must come from here: the checkers run one thread at a time and
//! a real block or spin inside a model stalls the run. Model parameters (`CAP`, `ARENA_CHUNK`)
//! live here too, so that the checkers see splits, node removals and chunk refills with a few
//! keys.

#[cfg(loom)]
mod mocked;
#[cfg(not(any(loom, shuttle)))]
mod real;
#[cfg(all(shuttle, not(loom)))]
mod shuttled;

#[cfg(loom)]
pub(crate) use mocked::*;
#[cfg(not(any(loom, shuttle)))]
pub(crate) use real::*;
#[cfg(all(shuttle, not(loom)))]
pub(crate) use shuttled::*;
