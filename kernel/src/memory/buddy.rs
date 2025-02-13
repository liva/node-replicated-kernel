//! A buddy allocator for managing physical memory.
//!
//! Some of this code was inspired by
//! https://crates.io/crates/alloc_buddy_simple (Apache2 / MIT License)
//!
//! # See also
//!   * https://en.wikipedia.org/wiki/Buddy_memory_allocation

use core::alloc::Layout;
use core::cmp::{max, min};
use core::fmt;
use core::ptr;

use crate::prelude::*;

use super::{
    AllocationError, AllocatorStatistics, DataSize, Frame, PAddr, PhysicalAllocator, VAddr,
    BASE_PAGE_SIZE, LARGE_PAGE_SIZE,
};
use crate::arch::memory::kernel_vaddr_to_paddr;

/// A free block in our heap.
pub struct FreeBlock {
    /// The next block in the free list, or NULL if this is the final
    /// block.
    next: *mut FreeBlock,
}

impl FreeBlock {
    /// Construct a `FreeBlock` header pointing at `next`.
    fn new(next: *mut FreeBlock) -> FreeBlock {
        FreeBlock { next: next }
    }
}

/// The interface to a heap.  This data structure is stored _outside_ the
/// heap somewhere, because every single byte of our heap is potentially
/// available for allocation.
pub struct BuddyFrameAllocator {
    /// The physical region managed by this allocator. Its base shall be aligned on a
    /// `min_heap_align` boundary (i.e., either 4 KiB or 2 MiB at the moment ).
    region: Frame,

    /// Total bytes currently allocated (in use)
    allocated_bytes: usize,

    /// Current internal fragmentation (bytes)
    internal_fragmentation: usize,

    /// The free lists for our heap.  The list at `free_lists[0]` contains
    /// the smallest block size we can allocate, and the list at the end
    /// can only contain a single free block the size of our entire heap,
    /// and only when no memory is allocated.
    free_lists: [*mut FreeBlock; 27],

    /// Our minimum block alignment (depends on region.base)
    min_heap_align: usize,

    /// Our minimum block size.
    min_block_size: usize,

    /// The log base 2 of our min block size.
    min_block_size_log2: u8,
}

impl BuddyFrameAllocator {
    pub fn new() -> BuddyFrameAllocator {
        BuddyFrameAllocator {
            region: Frame {
                base: PAddr(0),
                size: 0,
                affinity: 0,
            },
            allocated_bytes: 0,
            internal_fragmentation: 0,
            free_lists: [
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            ],
            min_heap_align: BASE_PAGE_SIZE,
            min_block_size: BASE_PAGE_SIZE,
            min_block_size_log2: 12,
        }
    }

    pub fn new_with_frame(f: Frame) -> BuddyFrameAllocator {
        let mut buddy = BuddyFrameAllocator::new();
        unsafe { assert!(buddy.add_memory(f)) };
        buddy
    }

    pub unsafe fn add_memory(&mut self, region: Frame) -> bool {
        if self.region.base.as_u64() == 0 {
            let size = region.size.next_power_of_two() >> 1;
            if size < region.size {
                let ret = DataSize::from_bytes(region.size - size);
                // split the frame and return the rest of it
                error!(
                    "TODO: Buddy only deals with powers-of-two, we lost {}.",
                    ret
                );
            }
            self.region.size = size;
            let order = self
                .layout_to_order(Layout::from_size_align_unchecked(size, 1))
                .expect("Failed to calculate order for root heap block");
            //trace!("order = {} size = {}", order, region.size);
            self.region.affinity = region.affinity;

            self.min_heap_align = if region.base.as_usize() % LARGE_PAGE_SIZE == 0 {
                LARGE_PAGE_SIZE
            } else {
                BASE_PAGE_SIZE
            };

            self.free_list_insert(order, region.kernel_vaddr().as_mut_ptr::<FreeBlock>());
            true
        } else {
            false
        }
    }

    /// Create a new heap.
    ///
    /// * `heap_base` must be aligned on a 4 KiB or 2 MiB boundary
    /// * `heap_size` must be a power of 2
    /// * `heap_size / 2 ** (free_lists.len()-1)` must be greater than or equal to `size_of::<FreeBlock>()`.
    #[cfg(test)]
    pub unsafe fn new_test_instance(region: Frame, min_block_size: usize) -> BuddyFrameAllocator {
        assert!(region.base.as_u64() > (BASE_PAGE_SIZE as u64));
        assert!(region.size.is_power_of_two());
        assert_eq!(region.base % BASE_PAGE_SIZE, 0);

        // TODO: this should be sized based on heap_size?
        // 27 with a min block size of 2**12 gives blocks of up to 512 GiB
        let free_list = [
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
        ];

        // We must have at least one free list
        assert!(free_list.len() > 0);

        // The heap must be big enough to contain at least one block.
        assert!(region.size >= min_block_size);

        // The smallest possible heap block must be big enough to contain
        // the block header.
        assert!(min_block_size >= core::mem::size_of::<FreeBlock>());

        // We must have one free list per possible heap block size.
        assert!(min_block_size * (2u32.pow(free_list.len() as u32 - 1)) as usize >= region.size);

        let min_heap_align = if region.base.as_usize() % LARGE_PAGE_SIZE == 0 {
            LARGE_PAGE_SIZE
        } else {
            BASE_PAGE_SIZE
        };

        let mut result = BuddyFrameAllocator {
            region: region,
            allocated_bytes: 0,
            internal_fragmentation: 0,
            free_lists: free_list,
            min_heap_align,
            min_block_size,
            min_block_size_log2: min_block_size.log2(),
        };

        // Insert the memory
        let order = result
            .layout_to_order(Layout::from_size_align_unchecked(region.size, 1))
            .expect("Failed to calculate order for root heap block");
        result.free_list_insert(order, region.kernel_vaddr().as_mut_ptr::<FreeBlock>());

        result
    }

    /// Get block size for allocation request.
    fn allocation_size(&self, layout: Layout) -> Option<usize> {
        if layout.align() > self.min_heap_align {
            trace!("Don't try to align more than our heap base alignment");
            return None;
        }

        // We're automatically aligned to `size` because of how our heap is
        // sub-divided, but if we need a larger alignment, we can only do
        // it be allocating more memory.
        let mut size = max(layout.size(), layout.align());
        // We can't allocate blocks smaller than `min_block_size`.
        size = max(size, self.min_block_size);
        // Round up to the next power of two.
        size = size.next_power_of_two();

        // We can't allocate a block bigger than our heap.
        if size <= self.region.size {
            Some(size)
        } else {
            trace!("We can't allocate a block bigger than our heap.");
            None
        }
    }

    /// The "order" of an allocation is how many times we need to double
    /// `min_block_size` in order to get a large enough block, as well as
    /// the index we use into `free_lists`.
    fn layout_to_order(&self, layout: Layout) -> Option<usize> {
        self.allocation_size(layout)
            .map(|s| (s.log2() - self.min_block_size_log2) as usize)
    }

    /// Calculate size for a given order (2^order).
    fn order_to_size(&self, order: usize) -> usize {
        1 << (self.min_block_size_log2 as usize + order)
    }

    /// Return first block off the appropriate free list.
    unsafe fn free_list_pop(&mut self, order: usize) -> Option<*mut FreeBlock> {
        let candidate = self.free_lists[order];
        if candidate != ptr::null_mut() {
            self.free_lists[order] = (*candidate).next;
            Some(candidate as *mut FreeBlock)
        } else {
            None
        }
    }

    /// Insert block in the corresponding free list slot.
    unsafe fn free_list_insert(&mut self, order: usize, free_block_ptr: *mut FreeBlock) {
        assert!(!free_block_ptr.is_null());
        *free_block_ptr = FreeBlock::new(self.free_lists[order]);
        self.free_lists[order] = free_block_ptr;
    }

    /// Attempt to remove a block from our free list, returning true
    /// success, and false if the block wasn't on our free list.
    unsafe fn free_list_remove(&mut self, order: usize, block_ptr: *mut FreeBlock) -> bool {
        // `*checking` is the pointer we want to check, and `checking` is
        // the memory location we found it at, which we'll need if we want
        // to replace the value `*checking` with a new value.
        let mut checking: *mut *mut FreeBlock = &mut self.free_lists[order];

        while *checking != ptr::null_mut() {
            // Is this the pointer we want to remove from the free list?
            if *checking == block_ptr {
                // Remove block from list
                *checking = (*(*checking)).next;
                return true;
            }
            checking = &mut ((*(*checking)).next);
        }

        false
    }

    /// Split a `block` of order `order` down into a block of order
    /// `order_needed`, placing any unused chunks on the free list.
    unsafe fn split_free_block(
        &mut self,
        block: *mut FreeBlock,
        mut order: usize,
        order_needed: usize,
    ) {
        let mut size_to_split = self.order_to_size(order);

        // Progressively cut our block down to size.
        while order > order_needed {
            // Update our loop counters to describe a block half the size.
            size_to_split >>= 1;
            order -= 1;

            // Insert the "upper half" of the block into the free list.
            let split = (block as *mut u8).offset(size_to_split as isize);
            self.free_list_insert(order, split as *mut FreeBlock);
        }
    }

    /// Given a `block` with the specified `order`, find the block
    /// we could potentially merge it with.
    pub unsafe fn buddy(&self, order: usize, block: *mut FreeBlock) -> Option<*mut FreeBlock> {
        let relative: usize = (block as usize) - (self.region.kernel_vaddr().as_usize());
        let size = self.order_to_size(order);
        if size >= self.region.size as usize {
            // The main heap itself does not have a budy.
            None
        } else {
            // We can find our buddy by XOR'ing the right bit in our
            // offset from the base of the heap.
            Some(
                self.region
                    .kernel_vaddr()
                    .as_mut_ptr::<u8>()
                    .offset((relative ^ size) as isize) as *mut FreeBlock,
            )
        }
    }
}

impl fmt::Debug for BuddyFrameAllocator {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let cap = DataSize::from_bytes(self.capacity());
        let free = DataSize::from_bytes(self.free());
        let allocd = DataSize::from_bytes(self.allocated());
        let frag = DataSize::from_bytes(self.internal_fragmentation());

        write!(
            f,
            "BuddyFrameAllocator {{ region: {:#x} -- {:#x}, cap: {}, free: {}, allocated: {}, internal_fragmentation: {} }}",
            self.region.base,
            self.region.end(),
            cap, free, allocd, frag
        )
    }
}

unsafe impl Send for BuddyFrameAllocator {}

impl PhysicalAllocator for BuddyFrameAllocator {
    /// Allocate a block of physical memory large enough to contain `size` bytes,
    /// and aligned on `align`.
    ///
    /// Returns None in case the request can not be satisfied.
    ///
    /// All allocated Frames must be passed to `deallocate` with the same
    /// `size` and `align` parameter.
    unsafe fn allocate_frame(&mut self, layout: Layout) -> Result<Frame, AllocationError> {
        trace!("buddy allocate {:?}", layout);
        // Figure out which order block we need.
        if let Some(order_needed) = self.layout_to_order(layout) {
            // Start with the smallest acceptable block size, and search
            // upwards until we reach blocks the size of the entire heap.
            for order in order_needed..self.free_lists.len() {
                // Do we have a block of this size?
                if let Some(block) = self.free_list_pop(order) {
                    // If the block is too big, break it up.  This leaves
                    // the address unchanged, because we always allocate at
                    // the head of a block.
                    if order > order_needed {
                        self.split_free_block(block, order, order_needed);
                    }

                    let f = Frame::const_new(
                        PAddr::from(kernel_vaddr_to_paddr(VAddr::from(block as usize))),
                        self.order_to_size(order_needed),
                        self.region.affinity,
                    );
                    self.allocated_bytes += f.size();
                    self.internal_fragmentation += f.size() - layout.size();
                    return Ok(f);
                }
            }
            trace!("Can't allocate in this order");
            Err(AllocationError::CacheExhausted)
        } else {
            trace!("Allocation size too big for request {:?}", layout);
            Err(AllocationError::InvalidLayout)
        }
    }

    /// Deallocate a block allocated using `allocate`.
    /// Layout value must match the value passed to
    /// `allocate`.
    unsafe fn deallocate_frame(&mut self, frame: Frame, layout: Layout) {
        trace!("buddy deallocate {:?} {:?}", frame, layout);
        let initial_order = self
            .layout_to_order(layout)
            .expect("Tried to dispose of invalid block");
        self.allocated_bytes -= frame.size();
        self.internal_fragmentation -= frame.size() - layout.size();

        // See if we can merge block with it's neighbouring buddy.
        // If so merge and continue walking up until done.
        //
        // `block` is the biggest merged block we have so far.
        let mut block = frame.kernel_vaddr().as_mut_ptr::<FreeBlock>();
        for order in initial_order..self.free_lists.len() {
            // Would this block have a buddy?
            if let Some(buddy) = self.buddy(order, block) {
                // Is this block's buddy free?
                if self.free_list_remove(order, buddy) {
                    // Merge them!  The lower address of the two is the
                    // newly-merged block.  Then we want to try again.
                    block = min(block, buddy);
                    continue;
                }
            }

            // If we reach here, we didn't find a buddy block of this size,
            // so take what we've got and mark it as free.
            self.free_list_insert(order, block);
            return;
        }
    }
}

impl AllocatorStatistics for BuddyFrameAllocator {
    fn allocated(&self) -> usize {
        self.allocated_bytes
    }

    fn size(&self) -> usize {
        self.region.size()
    }

    fn capacity(&self) -> usize {
        self.region.size()
    }

    fn internal_fragmentation(&self) -> usize {
        self.internal_fragmentation
    }
}

#[cfg(test)]
pub mod test {
    use super::*;

    use crate::alloc::alloc;
    use crate::arch::memory::{kernel_vaddr_to_paddr, VAddr};
    use core::alloc::Layout;

    #[test]
    fn test_allocation_size() {
        unsafe {
            let heap_size = 256;
            let mem = alloc::alloc(Layout::from_size_align_unchecked(heap_size, 4096));
            let pmem = kernel_vaddr_to_paddr(VAddr::from(mem as usize));
            let heap =
                BuddyFrameAllocator::new_test_instance(Frame::const_new(pmem, heap_size, 0), 16);
            let power_of_twos: [usize; 21] = [
                0,
                1,
                1 << 2,
                1 << 3,
                1 << 4,
                1 << 5,
                1 << 6,
                1 << 7,
                1 << 8,
                1 << 9,
                1 << 10,
                1 << 11,
                1 << 12,
                1 << 13,
                1 << 14,
                1 << 18,
                1 << 19,
                1 << 20,
                1 << 21,
                1 << 22,
                1 << 23,
            ];

            for size in 0..8192 {
                for align in power_of_twos.iter() {
                    // Simple allocations just round up to next block size.
                    // Aligned allocations use alignment as block size.
                    match Layout::from_size_align(size, *align) {
                        Ok(layout) => {
                            let res = heap.allocation_size(layout);
                            // Simple allocations just round up to next block size.
                            // Aligned allocations use alignment as block size.
                            let alloc_size = max(layout.size(), layout.align());
                            if alloc_size > heap_size {
                                // Can't align beyond heap_size.
                                assert_eq!(res, None);
                            } else {
                                // Can't go beyond minimum block size
                                let min_expected =
                                    max(alloc_size.next_power_of_two(), heap.min_block_size);
                                assert_eq!(Some(min_expected), res);
                            }
                        }
                        _ => continue,
                    }
                }
            }
        }
    }

    #[test]
    fn test_order() {
        unsafe {
            let heap_size = 256;
            let mem = alloc::alloc(Layout::from_size_align_unchecked(heap_size, 4096));
            let pmem = kernel_vaddr_to_paddr(VAddr::from(mem as usize));
            let heap =
                BuddyFrameAllocator::new_test_instance(Frame::const_new(pmem, heap_size, 0), 16);

            // Block orders.
            assert_eq!(
                Some(0),
                heap.layout_to_order(Layout::from_size_align(0, 1).unwrap())
            );
            assert_eq!(
                Some(0),
                heap.layout_to_order(Layout::from_size_align(1, 1).unwrap())
            );
            assert_eq!(
                Some(0),
                heap.layout_to_order(Layout::from_size_align(16, 16).unwrap())
            );
            assert_eq!(
                Some(1),
                heap.layout_to_order(Layout::from_size_align(32, 32).unwrap())
            );
            assert_eq!(
                Some(2),
                heap.layout_to_order(Layout::from_size_align(64, 64).unwrap())
            );
            assert_eq!(
                Some(3),
                heap.layout_to_order(Layout::from_size_align(128, 128).unwrap())
            );
            assert_eq!(
                Some(4),
                heap.layout_to_order(Layout::from_size_align(256, 256).unwrap())
            );
            assert_eq!(
                None,
                heap.layout_to_order(Layout::from_size_align(512, 512).unwrap())
            );
        }
    }

    #[test]
    fn test_buddy_one_page() {
        unsafe {
            let heap_size = BASE_PAGE_SIZE;
            let mem = alloc::alloc(Layout::from_size_align_unchecked(heap_size, BASE_PAGE_SIZE));
            let pmem = kernel_vaddr_to_paddr(VAddr::from(mem as usize));
            let heap =
                BuddyFrameAllocator::new_test_instance(Frame::const_new(pmem, heap_size, 0), 16);
        }
    }

    #[test]
    fn test_buddy() {
        unsafe {
            let heap_size = 256;
            let mem = alloc::alloc(Layout::from_size_align_unchecked(heap_size, 4096));
            let pmem = kernel_vaddr_to_paddr(VAddr::from(mem as usize));
            let heap =
                BuddyFrameAllocator::new_test_instance(Frame::const_new(pmem, heap_size, 0), 16);

            let block_16_0 = mem as *mut FreeBlock;
            let block_16_1 = mem.offset(16) as *mut FreeBlock;
            assert_eq!(Some(block_16_1), heap.buddy(0, block_16_0));
            assert_eq!(Some(block_16_0), heap.buddy(0, block_16_1));

            let block_32_0 = mem as *mut FreeBlock;
            let block_32_1 = mem.offset(32) as *mut FreeBlock;
            assert_eq!(Some(block_32_1), heap.buddy(1, block_32_0));
            assert_eq!(Some(block_32_0), heap.buddy(1, block_32_1));

            let block_32_2 = mem.offset(64) as *mut FreeBlock;
            let block_32_3 = mem.offset(96) as *mut FreeBlock;
            assert_eq!(Some(block_32_3), heap.buddy(1, block_32_2));
            assert_eq!(Some(block_32_2), heap.buddy(1, block_32_3));

            let block_256_0 = mem as *mut FreeBlock;
            assert_eq!(None, heap.buddy(4, block_256_0));
        }
    }

    /// Simple check that exercises most of the buddy system
    /// and also checks the `AllocatorStatistics` implementation for buddy.
    #[test]
    fn test_alloc_simple() {
        unsafe {
            let heap_size = 256;
            let mem = alloc::alloc(Layout::from_size_align_unchecked(heap_size, 4096));
            let pmem = kernel_vaddr_to_paddr(VAddr::from(mem as usize));
            let mut heap =
                BuddyFrameAllocator::new_test_instance(Frame::const_new(pmem, heap_size, 8), 16);
            assert_eq!(heap.size(), 256);
            assert_eq!(heap.capacity(), 256);
            assert_eq!(heap.internal_fragmentation(), 0);

            let block_16_0 = heap
                .allocate_frame(Layout::from_size_align_unchecked(8, 8))
                .unwrap();
            assert_eq!(mem as u64, block_16_0.base.as_u64());
            assert_eq!(heap.allocated(), 16);
            assert_eq!(heap.internal_fragmentation(), 8);
            assert_eq!(block_16_0.affinity, 8);

            let bigger_than_heap =
                heap.allocate_frame(Layout::from_size_align_unchecked(4096, heap_size));
            assert!(bigger_than_heap.is_err());

            let bigger_than_free =
                heap.allocate_frame(Layout::from_size_align_unchecked(heap_size, heap_size));
            assert!(bigger_than_free.is_err());
            assert_eq!(heap.allocated(), 16);
            assert_eq!(heap.internal_fragmentation(), 8);

            let block_16_1 = heap
                .allocate_frame(Layout::from_size_align_unchecked(8, 8))
                .unwrap();
            assert_eq!(mem.offset(16) as u64, block_16_1.base.as_u64());
            assert_eq!(heap.allocated(), 32);
            assert_eq!(heap.internal_fragmentation(), 16);
            assert_eq!(block_16_1.affinity, 8);

            let block_16_2 = heap
                .allocate_frame(Layout::from_size_align_unchecked(8, 8))
                .unwrap();
            assert_eq!(mem.offset(32) as u64, block_16_2.base.as_u64());
            assert_eq!(heap.allocated(), 48);
            assert_eq!(heap.internal_fragmentation(), 24);

            let block_32_2 = heap
                .allocate_frame(Layout::from_size_align_unchecked(32, 32))
                .unwrap();
            assert_eq!(mem.offset(64) as u64, block_32_2.base.as_u64());
            assert_eq!(heap.allocated(), 80);
            assert_eq!(heap.internal_fragmentation(), 24);

            let block_16_3 = heap
                .allocate_frame(Layout::from_size_align_unchecked(8, 8))
                .unwrap();
            assert_eq!(mem.offset(48) as u64, block_16_3.base.as_u64());
            assert_eq!(heap.allocated(), 96);
            assert_eq!(heap.internal_fragmentation(), 32);

            let block_128_1 = heap
                .allocate_frame(Layout::from_size_align_unchecked(128, 128))
                .unwrap();
            assert_eq!(mem.offset(128) as u64, block_128_1.base.as_u64());
            assert_eq!(heap.allocated(), 224);
            assert_eq!(heap.internal_fragmentation(), 32);

            let too_fragmented = heap.allocate_frame(Layout::from_size_align_unchecked(64, 64));
            assert!(too_fragmented.is_err());
            assert_eq!(heap.allocated(), 224);
            assert_eq!(heap.internal_fragmentation(), 32);

            heap.deallocate_frame(block_32_2, Layout::from_size_align_unchecked(32, 32));
            heap.deallocate_frame(block_16_0, Layout::from_size_align_unchecked(8, 8));
            heap.deallocate_frame(block_16_3, Layout::from_size_align_unchecked(8, 8));
            heap.deallocate_frame(block_16_1, Layout::from_size_align_unchecked(8, 8));
            heap.deallocate_frame(block_16_2, Layout::from_size_align_unchecked(8, 8));
            assert_eq!(heap.allocated(), 128);
            assert_eq!(heap.internal_fragmentation(), 0);

            let block_128_0 = heap
                .allocate_frame(Layout::from_size_align_unchecked(128, 128))
                .unwrap();
            assert_eq!(mem.offset(0) as u64, block_128_0.base.as_u64());
            assert_eq!(heap.allocated(), 256);
            assert_eq!(heap.internal_fragmentation(), 0);

            heap.deallocate_frame(block_128_1, Layout::from_size_align_unchecked(128, 128));
            heap.deallocate_frame(block_128_0, Layout::from_size_align_unchecked(128, 128));
            assert_eq!(heap.allocated(), 0);
            assert_eq!(heap.size(), 256);
            assert_eq!(heap.internal_fragmentation(), 0);

            // And allocate the whole heap, just to make sure everything
            // got cleaned up correctly.
            let block_256_0 = heap
                .allocate_frame(Layout::from_size_align_unchecked(256, 256))
                .unwrap();
            assert_eq!(mem.offset(0) as u64, block_256_0.base.as_u64());
            assert_eq!(heap.allocated(), 256);
            assert_eq!(heap.internal_fragmentation(), 0);
        }
    }

    macro_rules! test_allocation_single {
        ($test:ident, $size:expr, $alignment:expr, $allocations:expr) => {
            #[test]
            fn $test() {
                use crate::arch::memory::{kernel_vaddr_to_paddr, VAddr};

                use crate::alloc::alloc;
                use crate::alloc::vec::Vec;
                use core::arch::x86_64::_rdrand64_step;

                unsafe {
                    let heap_size = 4096;
                    let mem = alloc::alloc(Layout::from_size_align_unchecked(heap_size, 4096));
                    let mut rand: u64 = 0;
                    let pmem = kernel_vaddr_to_paddr(VAddr::from(mem as usize));
                    let mut heap = BuddyFrameAllocator::new_test_instance(
                        Frame::new(pmem, heap_size, 0),
                        BASE_PAGE_SIZE,
                    );

                    let alignment = $alignment;

                    let mut objects: Vec<(u64, Option<Frame>)> = Vec::new();
                    let layout = Layout::from_size_align($size, alignment).unwrap();

                    for _ in 0..$allocations {
                        let allocation = heap.allocate_frame(layout);
                        match allocation {
                            Ok(frame) => {
                                assert_eq!(1, _rdrand64_step(&mut rand));
                                objects.push((rand, Some(frame)));
                            }
                            Err(_) => objects.push((0, None)),
                        }
                    }

                    // Write the objects with a random pattern
                    for item in objects.iter_mut() {
                        let (pattern, mut frame) = *item;
                        frame.map(|mut f| {
                            f.fill(pattern);
                        });
                    }

                    for item in objects.iter() {
                        let (pattern, frame) = *item;
                        frame.map(|f| {
                            let obj: &[u64] = f.as_slice().unwrap();
                            for i in 0..obj.len() {
                                assert!(
                                    (obj[i]) == pattern,
                                    "No two allocations point to the same memory."
                                );
                            }
                        });
                    }

                    // Make sure we can correctly deallocate:
                    // Deallocate all the objects
                    let objects2 = objects.clone();
                    for (_rand, frame) in objects.into_iter() {
                        frame.map(|f| {
                            heap.deallocate_frame(f, Layout::from_size_align_unchecked(f.size, 1))
                        });
                    }

                    // then allocate everything again, should be deterministic (same as prev.)
                    for idx in 0..$allocations {
                        assert_eq!(objects2[idx].1, heap.allocate_frame(layout).ok());
                    }
                }
            }
        };
    }

    test_allocation_single!(test_allocation_single1, 8, 1, 14);
    test_allocation_single!(test_allocation_single2, 8, 8, 13);
    test_allocation_single!(test_allocation_single3, 16, 64, 4);
    test_allocation_single!(test_allocation_single4, 24, 1, 6);
    test_allocation_single!(test_allocation_single5, 32, 8, 7);
    test_allocation_single!(test_allocation_single6, 64, 64, 5);
    test_allocation_single!(test_allocation_single7, 71, 1, 3);
    test_allocation_single!(test_allocation_single8, 71, 1, 4);
    test_allocation_single!(test_allocation_single9, 256, 1, 2);

    #[test]
    fn random_size_allocation() {
        // A silly pattern right now, will start of with big chunks and run out of space quickly
        use crate::alloc::alloc;
        use crate::alloc::vec::Vec;
        use core::arch::x86_64::{_rdrand32_step, _rdrand64_step};

        unsafe {
            let alignment = 1;
            let allocations = 1024;
            let heap_size: usize = 128 * 1024 * 1024;
            assert!(heap_size.is_power_of_two());

            let mem = alloc::alloc(Layout::from_size_align_unchecked(heap_size, 4096));
            let pmem = kernel_vaddr_to_paddr(VAddr::from(mem as usize));
            let mut heap = BuddyFrameAllocator::new_test_instance(
                Frame::new(pmem, heap_size, 0),
                BASE_PAGE_SIZE,
            );

            let mut objects: Vec<(u64, Layout, Option<Frame>)> = Vec::new();

            for _ in 0..allocations {
                let mut random_size: u32 = 0;
                assert_eq!(1, _rdrand32_step(&mut random_size));
                let layout = Layout::from_size_align(
                    (random_size & ((heap_size >> 3) as u32)) as usize,
                    alignment,
                )
                .unwrap();

                let allocation = heap.allocate_frame(layout);

                match allocation {
                    Ok(frame) => {
                        debug!("Allocated {:?} random_size = {}", frame, random_size);
                        let mut rand_fill_pattern: u64 = 0;
                        assert_eq!(1, _rdrand64_step(&mut rand_fill_pattern));
                        objects.push((rand_fill_pattern, layout, Some(frame)));
                    }
                    Err(_) => objects.push((0, layout, None)),
                }
            }

            trace!("Write the objects with a random pattern");
            for item in objects.iter_mut() {
                let (pattern, _, mut frame) = *item;
                frame.map(|mut f| {
                    f.fill(pattern);
                });
            }

            trace!("Verify the pattern");
            for item in objects.iter() {
                let (pattern, _, frame) = *item;
                frame.map(|f| {
                    let obj: &[u64] = f.as_slice().unwrap();
                    for i in 0..obj.len() {
                        assert!(
                            (obj[i]) == pattern,
                            "No two allocations point to the same memory."
                        );
                    }
                });
            }

            trace!("Make sure we can correctly deallocate");
            // Deallocate all the objects
            let objects2 = objects.clone();
            for (_rand, _layout, frame) in objects.into_iter() {
                frame.map(|f| {
                    heap.deallocate_frame(f, Layout::from_size_align_unchecked(f.size, 1))
                });
            }

            // then allocate everything again, should be deterministic (same as prev.)
            for idx in 0..allocations {
                assert_eq!(objects2[idx].2, heap.allocate_frame(objects2[idx].1).ok());
            }
        }
    }

    /// Buddy should fit within a page for our code (we allocate a pages
    /// with correct NUMA locality and place buddies in there)
    #[test]
    fn buddy_is_less_than_page_sized() {
        assert!(core::mem::size_of::<BuddyFrameAllocator>() <= super::BASE_PAGE_SIZE);
    }

    #[test]
    /// Check that buddy takes on correct affinity from provided frame.
    fn buddy_affinity() {
        unsafe {
            let alignment = 1;
            let allocations = 1024;
            let heap_size: usize = 128 * 1024 * 1024;
            assert!(heap_size.is_power_of_two());

            let mem = alloc::alloc(Layout::from_size_align_unchecked(heap_size, 4096));
            let pmem = kernel_vaddr_to_paddr(VAddr::from(mem as usize));

            let mut heap = BuddyFrameAllocator::new_test_instance(
                Frame::new(pmem, heap_size, 3),
                BASE_PAGE_SIZE,
            );
            assert_eq!(heap.region.affinity, 3);

            let block_128_0 = heap
                .allocate_frame(Layout::from_size_align_unchecked(128, 128))
                .unwrap();
            assert_eq!(block_128_0.affinity, 3);
        }
    }

    #[test]
    /// Test that we can allocate a 2 MiB page from a large enough frame.
    fn can_alloc_two_mib() {
        unsafe {
            klogger::init(log::Level::Trace).expect("Logger init");

            let heap_size: usize = 128 * 1024 * 1024;
            assert!(heap_size.is_power_of_two());

            let mem = alloc::alloc(Layout::from_size_align_unchecked(
                heap_size,
                LARGE_PAGE_SIZE,
            ));
            let pmem = kernel_vaddr_to_paddr(VAddr::from(mem as usize));

            let mut heap = BuddyFrameAllocator::new_test_instance(
                Frame::new(pmem, heap_size, 3),
                BASE_PAGE_SIZE,
            );
            let b = heap.allocate_frame(Layout::from_size_align_unchecked(
                LARGE_PAGE_SIZE,
                LARGE_PAGE_SIZE,
            ));
            assert!(b.is_ok());
        }
    }

    #[test]
    /// Test that we can allocate a 2 MiB page from a large enough frame.
    fn two_mib_heap_exhaust() {
        unsafe {
            klogger::init(log::Level::Trace).expect("logger init");

            let heap_size: usize = 512 * 1024 * 1024;
            assert!(heap_size.is_power_of_two());

            let mem = alloc::alloc(Layout::from_size_align_unchecked(
                heap_size,
                LARGE_PAGE_SIZE,
            ));
            let pmem = kernel_vaddr_to_paddr(VAddr::from(mem as usize));

            let mut heap = BuddyFrameAllocator::new_test_instance(
                Frame::new(pmem, heap_size, 3),
                BASE_PAGE_SIZE,
            );

            for _ in 0..(heap_size / LARGE_PAGE_SIZE) {
                let b = heap.allocate_frame(Layout::from_size_align_unchecked(
                    LARGE_PAGE_SIZE,
                    LARGE_PAGE_SIZE,
                ));
                assert!(b.is_ok());
            }
        }
    }
}
