use crate::slab::{self, Family, MAX_HEIGHT};
use std::alloc::Layout;
use std::mem::{align_of, size_of};
use std::ptr::{self, NonNull};
use std::sync::Arc;

/// A forward link and the number of level-0 steps it spans. `width` is unused when `next` is null.
#[repr(C)]
pub(crate) struct Link<K, V> {
  pub(crate) next: *mut Node<K, V>,
  pub(crate) width: usize,
}

impl<K, V> Clone for Link<K, V> {
  fn clone(&self) -> Self {
    *self
  }
}

impl<K, V> Copy for Link<K, V> {}

impl<K, V> Link<K, V> {
  pub(crate) const EMPTY: Self = Link {
    next: ptr::null_mut(),
    width: 0,
  };
}

/// A node is one slot in its list's slab: `height` links laid out in reverse level order directly
/// before this header, so link 0 sits immediately before `key`.
///
/// The head uses the same addressing with `MAX_HEIGHT` links and its header fields are never read.
#[repr(C)]
pub(crate) struct Node<K, V> {
  pub(crate) key: K,
  pub(crate) prev: *const Node<K, V>,
  pub(crate) height: usize,
  pub(crate) value: V,
}

const LINK: usize = size_of::<Link<(), ()>>();

#[inline(always)]
fn layout<K, V>(height: usize) -> (Layout, usize) {
  let node = Layout::new::<Node<K, V>>();
  let align = if node.align() > align_of::<Link<K, V>>() {
    node.align()
  } else {
    align_of::<Link<K, V>>()
  };
  let offset = (LINK * height + align - 1) & !(align - 1);
  let layout = match Layout::from_size_align(offset + node.size(), align) {
    Ok(layout) => layout.pad_to_align(),
    Err(_) => panic!("skiplist node layout overflow"),
  };
  (layout, offset)
}

pub(crate) fn alloc_head<K, V>(family: Arc<Family>) -> NonNull<Node<K, V>> {
  slab::alloc_head(Link::<K, V>::EMPTY, family).cast()
}

/// Frees the head and the slab chunks only this list holds. Does not drop any key or value.
pub(crate) unsafe fn free_head<K, V>(head: *mut Node<K, V>) {
  unsafe { slab::free_head::<Link<K, V>>(head.cast()) }
}

pub(crate) unsafe fn slab_of<'a, K, V>(head: *mut Node<K, V>) -> &'a mut slab::Slab {
  unsafe { slab::slab(head.cast()) }
}

/// Takes a slot with `height` links from the head's slab. Neither the links nor the header are
/// initialised.
#[inline(always)]
pub(crate) unsafe fn alloc_node<K, V>(head: *mut Node<K, V>, height: usize) -> *mut Node<K, V> {
  unsafe {
    let (slot, offset) = layout::<K, V>(height);
    slab_of(head).alloc(height, slot).add(offset).cast()
  }
}

/// Returns a node's slot to the head's slab. Does not drop the key or the value.
#[inline(always)]
pub(crate) unsafe fn free_node<K, V>(head: *mut Node<K, V>, node: *mut Node<K, V>) {
  unsafe {
    let height = (*node).height;
    let (_, offset) = layout::<K, V>(height);
    slab_of(head).free(height, node.cast::<u8>().sub(offset));
  }
}

#[inline(always)]
pub(crate) unsafe fn link<K, V>(node: *mut Node<K, V>, level: usize) -> *mut Link<K, V> {
  unsafe { node.cast::<Link<K, V>>().sub(level + 1) }
}

#[inline(always)]
pub(crate) unsafe fn next<K, V>(node: *mut Node<K, V>, level: usize) -> *mut Node<K, V> {
  unsafe { (*link(node, level)).next }
}

/// Writes the header of a freshly allocated node.
#[inline(always)]
pub(crate) unsafe fn init_node<K, V>(node: *mut Node<K, V>, key: K, value: V, height: usize, prev: *const Node<K, V>) {
  unsafe {
    ptr::addr_of_mut!((*node).key).write(key);
    ptr::addr_of_mut!((*node).value).write(value);
    ptr::addr_of_mut!((*node).height).write(height);
    ptr::addr_of_mut!((*node).prev).write(prev);
  }
}

/// Moves the key and value out of a node without freeing its slot.
#[inline(always)]
pub(crate) unsafe fn read_node<K, V>(node: *mut Node<K, V>) -> (K, V) {
  unsafe { (ptr::addr_of!((*node).key).read(), ptr::addr_of!((*node).value).read()) }
}

/// Moves the key and value out of a node and returns its slot to the slab.
#[inline(always)]
pub(crate) unsafe fn take_node<K, V>(head: *mut Node<K, V>, node: *mut Node<K, V>) -> (K, V) {
  unsafe {
    let pair = read_node(node);
    free_node(head, node);
    pair
  }
}

/// Drops the key and value of a node without freeing its slot.
#[inline(always)]
pub(crate) unsafe fn drop_contents<K, V>(node: *mut Node<K, V>) {
  struct Value<V>(*mut V);
  impl<V> Drop for Value<V> {
    fn drop(&mut self) {
      unsafe { ptr::drop_in_place(self.0) }
    }
  }
  unsafe {
    let _value = Value(ptr::addr_of_mut!((*node).value));
    ptr::drop_in_place(ptr::addr_of_mut!((*node).key));
  }
}

/// Drops the key and value of a node and returns its slot to the slab.
#[inline]
pub(crate) unsafe fn drop_node<K, V>(head: *mut Node<K, V>, node: *mut Node<K, V>) {
  struct Free<K, V>(*mut Node<K, V>, *mut Node<K, V>);
  impl<K, V> Drop for Free<K, V> {
    fn drop(&mut self) {
      unsafe { free_node(self.0, self.1) }
    }
  }
  let _free = Free(head, node);
  unsafe { drop_contents(node) }
}

/// xorshift64 state for tower heights.
pub(crate) struct Levels(u64);

impl Levels {
  pub(crate) fn new() -> Self {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    Levels::seeded(RandomState::new().build_hasher().finish())
  }

  pub(crate) fn seeded(seed: u64) -> Self {
    Levels(seed | 1)
  }

  /// Height with P(h > n) = 4^-n, capped at `MAX_HEIGHT`.
  #[inline(always)]
  pub(crate) fn next(&mut self) -> usize {
    let mut x = self.0;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    self.0 = x;
    let height = 1 + (x.wrapping_mul(0x2545_F491_4F6C_DD1D).leading_zeros() as usize >> 1);
    if height > MAX_HEIGHT { MAX_HEIGHT } else { height }
  }

  /// A fresh word of the stream, for seeding a child list.
  pub(crate) fn fork(&mut self) -> u64 {
    self.next();
    self.0.wrapping_mul(0x9E37_79B9_7F4A_7C15)
  }
}
