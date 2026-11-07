#![doc = include_str!("../README.USAGE.md")]

mod node;
mod raw;
mod slab;

pub mod list;
pub mod map;
pub mod ordered;
pub mod set;

pub use list::SkipList;
pub use map::SkipMap;
pub use ordered::OrderedSkipList;
pub use set::SkipSet;

#[cfg(test)]
pub(crate) mod test_util {
  pub fn scaled<T: core::ops::Div<Output = T> + From<u8>>(n: T) -> T {
    if cfg!(miri) { n / T::from(20) } else { n }
  }
}
