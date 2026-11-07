use crate::slab::{self, Family, MAX_HEIGHT};
use std::alloc::Layout;
use std::marker::PhantomData;
use std::mem::{align_of, size_of};
use std::ptr::{self, NonNull};
use std::sync::Arc;

/// A forward link and the number of entries from this node's first entry to the first entry of
/// `next`. `width` is unused when `next` is null.
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

/// The header of a node. A node is one slot in its list's slab: `height` links laid out in
/// reverse level order directly before this header, then `CAP` keys, then `CAP` values, of which
/// the first `len` of each are initialised.
///
/// The head uses the same link addressing with `MAX_HEIGHT` links and its header is never read.
#[repr(C)]
pub(crate) struct Node<K, V> {
  pub(crate) prev: *const Node<K, V>,
  pub(crate) len: u32,
  pub(crate) height: u32,
  marker: PhantomData<(K, V)>,
}

/// Entries per node: about 512 bytes of keys and values, between 4 and 32 entries.
pub(crate) const fn cap<K, V>() -> usize {
  let entry = size_of::<K>() + size_of::<V>();
  if entry == 0 {
    return 32;
  }
  let c = 512 / entry;
  if c > 32 {
    32
  } else if c < 4 {
    4
  } else {
    c
  }
}

const fn round_up(x: usize, align: usize) -> usize {
  (x + align - 1) & !(align - 1)
}

const fn max(a: usize, b: usize) -> usize {
  if a > b { a } else { b }
}

const fn keys_offset<K, V>() -> usize {
  round_up(size_of::<Node<K, V>>(), align_of::<K>())
}

const fn vals_offset<K, V>() -> usize {
  round_up(keys_offset::<K, V>() + cap::<K, V>() * size_of::<K>(), align_of::<V>())
}

const fn body_size<K, V>() -> usize {
  vals_offset::<K, V>() + cap::<K, V>() * size_of::<V>()
}

const fn node_align<K, V>() -> usize {
  max(align_of::<Link<K, V>>(), max(align_of::<K>(), align_of::<V>()))
}

const LINK: usize = size_of::<Link<(), ()>>();

#[inline(always)]
fn layout<K, V>(height: usize) -> (Layout, usize) {
  let align = node_align::<K, V>();
  let offset = round_up(LINK * height, align);
  let layout = match Layout::from_size_align(offset + body_size::<K, V>(), align) {
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

/// Takes a slot with `height` links from the head's slab and writes its header with no entries.
/// The links are not initialised.
#[inline(always)]
pub(crate) unsafe fn alloc_node<K, V>(
  head: *mut Node<K, V>,
  height: usize,
  prev: *const Node<K, V>,
) -> *mut Node<K, V> {
  unsafe {
    let (slot, offset) = layout::<K, V>(height);
    let node: *mut Node<K, V> = slab_of(head).alloc(height, slot).add(offset).cast();
    ptr::addr_of_mut!((*node).prev).write(prev);
    ptr::addr_of_mut!((*node).len).write(0);
    ptr::addr_of_mut!((*node).height).write(height as u32);
    node
  }
}

/// Returns a node's slot to the head's slab. Does not drop any key or value.
#[inline(always)]
pub(crate) unsafe fn free_node<K, V>(head: *mut Node<K, V>, node: *mut Node<K, V>) {
  unsafe {
    let height = height(node);
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

#[inline(always)]
pub(crate) unsafe fn len<K, V>(node: *mut Node<K, V>) -> usize {
  unsafe { (*node).len as usize }
}

#[inline(always)]
pub(crate) unsafe fn set_len<K, V>(node: *mut Node<K, V>, len: usize) {
  unsafe { (*node).len = len as u32 }
}

#[inline(always)]
pub(crate) unsafe fn height<K, V>(node: *mut Node<K, V>) -> usize {
  unsafe { (*node).height as usize }
}

#[inline(always)]
pub(crate) unsafe fn keys<K, V>(node: *mut Node<K, V>) -> *mut K {
  unsafe { node.cast::<u8>().add(keys_offset::<K, V>()).cast() }
}

#[inline(always)]
pub(crate) unsafe fn vals<K, V>(node: *mut Node<K, V>) -> *mut V {
  unsafe { node.cast::<u8>().add(vals_offset::<K, V>()).cast() }
}

/// The initialised keys of a node.
#[inline(always)]
pub(crate) unsafe fn key_slice<'a, K, V>(node: *mut Node<K, V>) -> &'a [K] {
  unsafe { std::slice::from_raw_parts(keys(node), len(node)) }
}

/// Writes an entry at `slot`, which must be unused.
#[inline(always)]
pub(crate) unsafe fn write_entry<K, V>(node: *mut Node<K, V>, slot: usize, key: K, value: V) {
  unsafe {
    keys(node).add(slot).write(key);
    vals(node).add(slot).write(value);
  }
}

/// Moves the entry at `slot` out without changing `len`.
#[inline(always)]
pub(crate) unsafe fn read_entry<K, V>(node: *mut Node<K, V>, slot: usize) -> (K, V) {
  unsafe { (keys(node).add(slot).read(), vals(node).add(slot).read()) }
}

/// Moves `count` entries starting at `from` to start at `to` within one node.
#[inline(always)]
pub(crate) unsafe fn shift_entries<K, V>(node: *mut Node<K, V>, from: usize, to: usize, count: usize) {
  unsafe {
    ptr::copy(keys(node).add(from), keys(node).add(to), count);
    ptr::copy(vals(node).add(from), vals(node).add(to), count);
  }
}

/// Moves `count` entries starting at `from` in `src` to start at `to` in `dst`, a different node.
#[inline(always)]
pub(crate) unsafe fn move_entries<K, V>(
  src: *mut Node<K, V>,
  from: usize,
  dst: *mut Node<K, V>,
  to: usize,
  count: usize,
) {
  unsafe {
    ptr::copy_nonoverlapping(keys(src).add(from), keys(dst).add(to), count);
    ptr::copy_nonoverlapping(vals(src).add(from), vals(dst).add(to), count);
  }
}

/// Drops the entries of a node without freeing its slot or changing `len`.
pub(crate) unsafe fn drop_entries<K, V>(node: *mut Node<K, V>) {
  unsafe {
    let n = len(node);
    let keys = keys(node);
    let vals = vals(node);
    struct Rest<K, V>(*mut K, *mut V, usize);
    impl<K, V> Drop for Rest<K, V> {
      fn drop(&mut self) {
        unsafe {
          ptr::drop_in_place(ptr::slice_from_raw_parts_mut(self.0, self.2));
          ptr::drop_in_place(ptr::slice_from_raw_parts_mut(self.1, self.2));
        }
      }
    }
    let _rest = Rest(keys, vals, n);
  }
}

/// Drops the entry at `slot` without changing `len`.
#[inline(always)]
pub(crate) unsafe fn drop_entry<K, V>(node: *mut Node<K, V>, slot: usize) {
  struct Value<V>(*mut V);
  impl<V> Drop for Value<V> {
    fn drop(&mut self) {
      unsafe { ptr::drop_in_place(self.0) }
    }
  }
  unsafe {
    let _value = Value(vals(node).add(slot));
    ptr::drop_in_place(keys(node).add(slot));
  }
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

  /// Height with P(h > n) = 2^-n, capped at `MAX_HEIGHT`.
  #[inline(always)]
  pub(crate) fn next_half(&mut self) -> usize {
    let mut x = self.0;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    self.0 = x;
    let height = 1 + x.wrapping_mul(0x2545_F491_4F6C_DD1D).leading_zeros() as usize;
    if height > MAX_HEIGHT { MAX_HEIGHT } else { height }
  }

  /// A fresh word of the stream, for seeding a child list.
  pub(crate) fn fork(&mut self) -> u64 {
    self.next();
    self.0.wrapping_mul(0x9E37_79B9_7F4A_7C15)
  }
}
