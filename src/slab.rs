use parking_lot::Mutex;
use std::alloc::{Layout, alloc, dealloc, handle_alloc_error};
use std::mem::{ManuallyDrop, size_of};
use std::ptr::{self, NonNull};
use std::sync::Arc;

pub(crate) const MAX_HEIGHT: usize = 24;

/// Chunk memory shared by every list in one lineage of splits and appends.
///
/// After `split_off` or `append`, a list's nodes can sit in chunks another list of the lineage
/// allocated, so a chunk is freed only when every list holding the family has dropped. `parents`
/// keeps alive the families of lists that were appended into this lineage. `spare_heads` holds the
/// head blocks of dropped lists, slab state included, for the next list made in the lineage. The
/// locks are taken when a chunk is allocated, a lineage absorbs another or a list is made or
/// dropped, never on a node allocation or free.
#[derive(Default)]
pub(crate) struct Family {
  chunks: Mutex<Vec<(NonNull<u8>, Layout)>>,
  parents: Mutex<Vec<Arc<Family>>>,
  spare_heads: Mutex<Vec<NonNull<u8>>>,
}

unsafe impl Send for Family {}
unsafe impl Sync for Family {}

impl Drop for Family {
  fn drop(&mut self) {
    for block in self.spare_heads.get_mut().drain(..) {
      unsafe { drop(Box::from_raw(block.as_ptr().cast::<HeadBlock<RawLink>>())) }
    }
    for (chunk, layout) in self.chunks.get_mut().drain(..) {
      unsafe { dealloc(chunk.as_ptr(), layout) }
    }
  }
}

/// Node storage for one list: chunks of equal-sized slots per tower height, with a free list per
/// height threaded through the first word of each freed slot.
pub(crate) struct Slab {
  free: [*mut u8; MAX_HEIGHT],
  bump: [*mut u8; MAX_HEIGHT],
  end: [*mut u8; MAX_HEIGHT],
  grow: [u8; MAX_HEIGHT],
  family: ManuallyDrop<Arc<Family>>,
}

impl Slab {
  fn new(family: Arc<Family>) -> Self {
    Slab {
      free: [ptr::null_mut(); MAX_HEIGHT],
      bump: [ptr::null_mut(); MAX_HEIGHT],
      end: [ptr::null_mut(); MAX_HEIGHT],
      grow: [0; MAX_HEIGHT],
      family: ManuallyDrop::new(family),
    }
  }

  pub(crate) fn family(&self) -> &Arc<Family> {
    &self.family
  }

  /// Takes a slot of layout `slot` for a node of `height` links and returns its base address.
  #[inline(always)]
  pub(crate) unsafe fn alloc(&mut self, height: usize, slot: Layout) -> *mut u8 {
    unsafe {
      let index = height - 1;
      let base = *self.free.get_unchecked(index);
      if !base.is_null() {
        *self.free.get_unchecked_mut(index) = *base.cast::<*mut u8>();
        return base;
      }
      if *self.bump.get_unchecked(index) == *self.end.get_unchecked(index) {
        self.refill(index, slot);
      }
      let base = *self.bump.get_unchecked(index);
      *self.bump.get_unchecked_mut(index) = base.add(slot.size());
      base
    }
  }

  /// Returns the slot at `base`, taken for a node of `height` links, to the free list.
  #[inline(always)]
  pub(crate) unsafe fn free(&mut self, height: usize, base: *mut u8) {
    unsafe {
      *base.cast::<*mut u8>() = *self.free.get_unchecked(height - 1);
      *self.free.get_unchecked_mut(height - 1) = base;
    }
  }

  #[cold]
  #[inline(never)]
  fn refill(&mut self, index: usize, slot: Layout) {
    let slots = 8usize << self.grow[index];
    if self.grow[index] < 7 {
      self.grow[index] += 1;
    }
    let layout = match Layout::from_size_align(slot.size() * slots, slot.align()) {
      Ok(layout) => layout,
      Err(_) => panic!("skiplist chunk layout overflow"),
    };
    let chunk = unsafe { alloc(layout) };
    let Some(chunk) = NonNull::new(chunk) else {
      handle_alloc_error(layout)
    };
    self.family.chunks.lock().push((chunk, layout));
    self.bump[index] = chunk.as_ptr();
    self.end[index] = unsafe { chunk.as_ptr().add(layout.size()) };
  }

  /// Keeps `other`'s chunks alive for as long as this slab's family lives, for nodes that moved
  /// from `other`'s list into this one. When no third list shares `other`'s family its chunks move
  /// into this family outright and `other` starts allocating afresh.
  pub(crate) fn absorb(&mut self, other: &mut Slab) {
    if Arc::ptr_eq(&self.family, &other.family) {
      return;
    }
    if Arc::strong_count(&other.family) == 1 {
      self.family.chunks.lock().extend(other.family.chunks.lock().drain(..));
      self.family.parents.lock().extend(other.family.parents.lock().drain(..));
      self
        .family
        .spare_heads
        .lock()
        .extend(other.family.spare_heads.lock().drain(..));
      other.free = [ptr::null_mut(); MAX_HEIGHT];
      other.bump = [ptr::null_mut(); MAX_HEIGHT];
      other.end = [ptr::null_mut(); MAX_HEIGHT];
    } else {
      let mut parents = self.family.parents.lock();
      if !parents.iter().any(|p| Arc::ptr_eq(p, &other.family)) {
        parents.push(Arc::clone(&other.family));
      }
    }
  }

  /// Forgets every slot. Frees the chunks when no other list shares the family; otherwise they stay
  /// allocated until the lineage drops.
  pub(crate) fn reset(&mut self) {
    if Arc::strong_count(&self.family) == 1 {
      unsafe { ManuallyDrop::drop(&mut self.family) }
      self.family = ManuallyDrop::new(Arc::default());
      self.grow = [0; MAX_HEIGHT];
    }
    self.free = [ptr::null_mut(); MAX_HEIGHT];
    self.bump = [ptr::null_mut(); MAX_HEIGHT];
    self.end = [ptr::null_mut(); MAX_HEIGHT];
  }
}

/// `MAX_HEIGHT` head links of type `L` followed by the slab. The head pointer is the address where
/// the links end, which is also the address of the slab.
///
/// Every `L` the crate uses is two words, so a block allocated for one `L` has the layout of
/// `HeadBlock<RawLink>` and can be freed or reused as that.
#[repr(C)]
struct HeadBlock<L> {
  links: [L; MAX_HEIGHT],
  slab: Slab,
}

type RawLink = [*mut u8; 2];

/// Makes a head block whose slab allocates from `family`, reusing a spare block of the family when
/// there is one.
pub(crate) fn alloc_head<L: Copy>(empty: L, family: Arc<Family>) -> NonNull<u8> {
  const { assert!(size_of::<L>() == size_of::<RawLink>()) };
  let spare = family.spare_heads.lock().pop();
  let block = match spare {
    Some(block) => unsafe {
      let block = block.as_ptr().cast::<HeadBlock<L>>();
      (*block).links = [empty; MAX_HEIGHT];
      ptr::addr_of_mut!((*block).slab.family).write(ManuallyDrop::new(family));
      block
    },
    None => Box::into_raw(Box::new(HeadBlock {
      links: [empty; MAX_HEIGHT],
      slab: Slab::new(family),
    })),
  };
  unsafe { NonNull::new_unchecked(ptr::addr_of_mut!((*block).slab).cast()) }
}

/// Releases the head block. While another list shares the family the block is kept as a spare for
/// the lineage, slab state included, since its free slots are unused memory in chunks the family
/// still holds. Chunks shared with another list stay alive until that list drops too.
pub(crate) unsafe fn free_head<L>(head: *mut u8) {
  unsafe {
    let block = head.sub(size_of::<[L; MAX_HEIGHT]>());
    let family = ManuallyDrop::take(&mut slab(head).family);
    if Arc::strong_count(&family) > 1 {
      family.spare_heads.lock().push(NonNull::new_unchecked(block));
      drop(family);
    } else {
      drop(family);
      drop(Box::from_raw(block.cast::<HeadBlock<L>>()));
    }
  }
}

#[inline(always)]
pub(crate) unsafe fn slab<'a>(head: *mut u8) -> &'a mut Slab {
  unsafe { &mut *head.cast::<Slab>() }
}
