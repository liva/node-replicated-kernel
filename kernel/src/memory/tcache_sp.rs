//! TCacheSp is identical to the TCache except that it has
//! a lot more 4K pages available.
//!
//! This is useful for the early memory manager on core 0
//! and needs to allocate a lot of small objects )thanks to ACPI).
//!
//! TODO(code-duplication): Ideally we should instantiate with some macros
//! or wait till ArrayVec allows us to dynamically define array sizes?

use super::*;

/// A simple page-cache for a CPU thread.
///
/// Holds two stacks of pages for O(1) allocation/deallocation.
/// Implements the `ReapBackend` to give pages back.
pub struct TCacheSp {
    /// Which node the memory in this cache is from.
    node: topology::NodeId,
    /// A vector of free, cached base-page addresses.
    base_page_addresses: arrayvec::ArrayVec<[PAddr; 2048]>,
    /// A vector of free, cached large-page addresses.
    large_page_addresses: arrayvec::ArrayVec<[PAddr; 12]>,
}

impl crate::kcb::MemManager for TCacheSp {}

impl TCacheSp {
    pub fn new(_thread: topology::ThreadId, node: topology::NodeId) -> TCacheSp {
        TCacheSp {
            node,
            base_page_addresses: arrayvec::ArrayVec::new(),
            large_page_addresses: arrayvec::ArrayVec::new(),
        }
    }

    pub fn new_with_frame(
        thread: topology::ThreadId,
        node: topology::NodeId,
        mem: Frame,
    ) -> TCacheSp {
        let mut tcache = TCacheSp::new(thread, node);
        tcache.populate(mem);
        tcache
    }

    /// Populates a TCacheSp with the memory from `frame`
    ///
    /// This works by repeatedly splitting the `frame`
    /// into smaller pages.
    fn populate(&mut self, frame: Frame) {
        let mut how_many_large_pages = if frame.base_pages() > self.base_page_addresses.capacity() {
            let bytes_left_after_base_full =
                (frame.base_pages() - self.base_page_addresses.capacity()) * BASE_PAGE_SIZE;
            bytes_left_after_base_full / LARGE_PAGE_SIZE
        } else {
            // If this assert fails, we have to rethink what to return here
            debug_assert!(self.base_page_addresses.capacity() * BASE_PAGE_SIZE <= LARGE_PAGE_SIZE);
            1
        };
        if how_many_large_pages == 0 {
            // XXX: Try to have at least one large-page if possible
            how_many_large_pages = 1;
        }

        let (low_frame, mut large_page_aligned_frame) =
            frame.split_at_nearest_large_page_boundary();

        for base_page in low_frame.into_iter() {
            self.base_page_addresses
                .try_push(base_page.base)
                .expect("Can't add base-page from low_frame to TCacheSp");
        }

        // Add large-pages
        while how_many_large_pages > 0 && large_page_aligned_frame.size() >= LARGE_PAGE_SIZE {
            let (large_page, rest) = large_page_aligned_frame.split_at(LARGE_PAGE_SIZE);
            self.large_page_addresses
                .try_push(large_page.base)
                .expect("Can't push large page in TCacheSp");

            large_page_aligned_frame = rest;
            how_many_large_pages -= 1;
        }

        // Put the rest as base-pages
        let mut lost_pages = 0;
        for base_page in large_page_aligned_frame.into_iter() {
            match self.base_page_addresses.try_push(base_page.base) {
                Ok(()) => continue,
                Err(_) => {
                    lost_pages += 1;
                }
            }
        }

        if lost_pages > 0 {
            debug!(
                "TCacheSp population lost {} of memory",
                DataSize::from_bytes(lost_pages * BASE_PAGE_SIZE)
            );
        }

        debug!(
            "TCacheSp populated with {} base-pages and {} large-pages",
            self.base_page_addresses.len(),
            self.large_page_addresses.len()
        );
    }

    fn paddr_to_base_page(&self, pa: PAddr) -> Frame {
        Frame::new(pa, BASE_PAGE_SIZE, self.node)
    }

    fn paddr_to_large_page(&self, pa: PAddr) -> Frame {
        Frame::new(pa, LARGE_PAGE_SIZE, self.node)
    }
}

impl AllocatorStatistics for TCacheSp {
    /// How much free memory (bytes) we have left.
    fn free(&self) -> usize {
        self.base_page_addresses.len() * BASE_PAGE_SIZE
            + self.large_page_addresses.len() * LARGE_PAGE_SIZE
    }

    /// How much free memory we can maintain.
    fn capacity(&self) -> usize {
        self.base_page_addresses.capacity() * BASE_PAGE_SIZE
            + self.large_page_addresses.capacity() * LARGE_PAGE_SIZE
    }

    fn allocated(&self) -> usize {
        0
    }

    fn size(&self) -> usize {
        0
    }

    fn internal_fragmentation(&self) -> usize {
        0
    }

    /// How many basepages we can allocate from the cache.
    fn free_base_pages(&self) -> usize {
        self.base_page_addresses.len()
    }

    /// How many large-pages we can allocate from the cache.
    fn free_large_pages(&self) -> usize {
        self.large_page_addresses.len()
    }
}

impl fmt::Debug for TCacheSp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TCacheSp")
            .field("free", &self.free())
            .field("capacity", &self.capacity())
            .field("allocated", &self.allocated())
            .finish()
    }
}

impl PhysicalPageProvider for TCacheSp {
    fn allocate_base_page(&mut self) -> Result<Frame, AllocationError> {
        let paddr = self
            .base_page_addresses
            .pop()
            .ok_or(AllocationError::CacheExhausted)?;
        Ok(self.paddr_to_base_page(paddr))
    }

    fn release_base_page(&mut self, frame: Frame) -> Result<(), AllocationError> {
        assert_eq!(frame.size(), BASE_PAGE_SIZE);
        assert_eq!(frame.base % BASE_PAGE_SIZE, 0);
        assert_eq!(frame.affinity, self.node);

        self.base_page_addresses
            .try_push(frame.base)
            .map_err(|_e| AllocationError::CacheFull)
    }

    fn allocate_large_page(&mut self) -> Result<Frame, AllocationError> {
        let paddr = self
            .large_page_addresses
            .pop()
            .ok_or(AllocationError::CacheExhausted)?;
        Ok(self.paddr_to_large_page(paddr))
    }

    fn release_large_page(&mut self, frame: Frame) -> Result<(), AllocationError> {
        assert_eq!(frame.size(), LARGE_PAGE_SIZE);
        assert_eq!(frame.base % LARGE_PAGE_SIZE, 0);
        assert_eq!(frame.affinity, self.node);

        self.large_page_addresses
            .try_push(frame.base)
            .map_err(|_e| AllocationError::CacheFull)
    }
}

impl ReapBackend for TCacheSp {
    /// Give base-pages back.
    fn reap_base_pages(&mut self, free_list: &mut [Option<Frame>]) {
        for insert in free_list.iter_mut() {
            if let Some(paddr) = self.base_page_addresses.pop() {
                *insert = Some(self.paddr_to_base_page(paddr));
            } else {
                // We don't have anything left in our cache
                break;
            }
        }
    }

    /// Give large-pages back.
    fn reap_large_pages(&mut self, free_list: &mut [Option<Frame>]) {
        for insert in free_list.iter_mut() {
            if let Some(paddr) = self.large_page_addresses.pop() {
                *insert = Some(self.paddr_to_large_page(paddr));
            } else {
                // We don't have anything left in our cache
                break;
            }
        }
    }
}

impl GrowBackend for TCacheSp {
    fn base_page_capcacity(&self) -> usize {
        self.base_page_addresses.capacity() - self.base_page_addresses.len()
    }

    fn grow_base_pages(&mut self, free_list: &[Frame]) -> Result<(), AllocationError> {
        for frame in free_list {
            assert_eq!(frame.size(), BASE_PAGE_SIZE);
            assert_eq!(frame.base % BASE_PAGE_SIZE, 0);
            assert_eq!(frame.affinity, self.node);

            self.base_page_addresses
                .try_push(frame.base)
                .map_err(|_e| AllocationError::CacheFull)?;
        }
        Ok(())
    }

    fn large_page_capcacity(&self) -> usize {
        self.large_page_addresses.capacity() - self.large_page_addresses.len()
    }

    /// Add a slice of large-pages to `self`.
    fn grow_large_pages(&mut self, free_list: &[Frame]) -> Result<(), AllocationError> {
        for frame in free_list {
            assert_eq!(frame.size(), LARGE_PAGE_SIZE);
            assert_eq!(frame.base % LARGE_PAGE_SIZE, 0);
            assert_eq!(frame.affinity, self.node);

            self.large_page_addresses
                .try_push(frame.base)
                .map_err(|_e| AllocationError::CacheFull)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    /// Can't add wrong size.
    #[test]
    #[should_panic]
    fn tcache_sp_invalid_base_frame_size() {
        let mut tcache = TCacheSp::new(1, 4);
        tcache
            .release_base_page(Frame::new(PAddr::from(0x2000), 0x1001, 4))
            .expect("release");
    }

    /// Can't add wrong size.
    #[test]
    #[should_panic]
    fn tcache_sp_invalid_base_frame_align() {
        let mut tcache = TCacheSp::new(1, 4);
        tcache
            .release_base_page(Frame::new(PAddr::from(0x2001), 0x1000, 4))
            .expect("release");
    }

    /// Can't add wrong affinity.
    #[test]
    #[should_panic]
    fn tcache_sp_invalid_affinity() {
        let mut tcache = TCacheSp::new(1, 1);
        tcache
            .release_base_page(Frame::new(PAddr::from(0x2000), 0x1000, 4))
            .expect("release");
    }

    /// Test that reap interface of the tcache.
    #[test]
    fn tcache_sp_reap() {
        let mut tcache = TCacheSp::new(1, 4);

        // Insert some pages
        tcache
            .release_base_page(Frame::new(PAddr::from(0x2000), 0x1000, 4))
            .expect("release");
        tcache
            .release_base_page(Frame::new(PAddr::from(0x3000), 0x1000, 4))
            .expect("release");

        tcache
            .release_large_page(Frame::new(PAddr::from(LARGE_PAGE_SIZE), LARGE_PAGE_SIZE, 4))
            .expect("release");
        tcache
            .release_large_page(Frame::new(
                PAddr::from(LARGE_PAGE_SIZE * 4),
                LARGE_PAGE_SIZE,
                4,
            ))
            .expect("release");

        let mut free_list = [None];
        tcache.reap_base_pages(&mut free_list);
        assert_eq!(free_list[0].unwrap().base.as_u64(), 0x3000);
        assert_eq!(free_list[0].unwrap().size, 0x1000);
        assert_eq!(free_list[0].unwrap().affinity, 4);

        let mut free_list = [None, None, None];
        tcache.reap_base_pages(&mut free_list);
        assert_eq!(free_list[0].unwrap().base.as_u64(), 0x2000);
        assert_eq!(free_list[0].unwrap().size, 0x1000);
        assert_eq!(free_list[0].unwrap().affinity, 4);
        assert!(free_list[1].is_none());
        assert!(free_list[2].is_none());

        let mut free_list = [None, None];
        tcache.reap_large_pages(&mut free_list);
        assert_eq!(free_list[0].unwrap().base.as_usize(), LARGE_PAGE_SIZE * 4);
        assert_eq!(free_list[0].unwrap().size, LARGE_PAGE_SIZE);
        assert_eq!(free_list[0].unwrap().affinity, 4);
        assert_eq!(free_list[1].unwrap().base.as_usize(), LARGE_PAGE_SIZE);
        assert_eq!(free_list[1].unwrap().size, LARGE_PAGE_SIZE);
        assert_eq!(free_list[1].unwrap().affinity, 4);
    }

    /// Test that release and allocate works as expected.
    /// Also verify free memory reporting along the way.
    #[test]
    fn tcache_sp_release_allocate() {
        let mut tcache = TCacheSp::new(1, 2);

        // Insert some pages
        tcache
            .release_base_page(Frame::new(PAddr::from(0x2000), 0x1000, 2))
            .expect("release");
        tcache
            .release_base_page(Frame::new(PAddr::from(0x3000), 0x1000, 2))
            .expect("release");

        tcache
            .release_large_page(Frame::new(PAddr::from(LARGE_PAGE_SIZE), LARGE_PAGE_SIZE, 2))
            .expect("release");
        tcache
            .release_large_page(Frame::new(
                PAddr::from(LARGE_PAGE_SIZE * 2),
                LARGE_PAGE_SIZE,
                2,
            ))
            .expect("release");
        assert_eq!(tcache.free(), 2 * BASE_PAGE_SIZE + 2 * LARGE_PAGE_SIZE);

        // Can we allocate
        let f = tcache.allocate_base_page().expect("Can allocate");
        assert_eq!(f.base.as_u64(), 0x3000);
        assert_eq!(f.size, 0x1000);
        assert_eq!(f.affinity, 2);

        let f = tcache.allocate_base_page().expect("Can allocate");
        assert_eq!(f.base.as_u64(), 0x2000);
        assert_eq!(f.size, 0x1000);
        assert_eq!(f.affinity, 2);

        let _f = tcache
            .allocate_base_page()
            .expect_err("Can't allocate more than we gave it");

        assert_eq!(tcache.free(), 2 * LARGE_PAGE_SIZE);

        let f = tcache.allocate_large_page().expect("Can allocate");
        assert_eq!(f.base.as_u64(), (LARGE_PAGE_SIZE * 2) as u64);
        assert_eq!(f.size, LARGE_PAGE_SIZE);
        assert_eq!(f.affinity, 2);

        let f = tcache.allocate_large_page().expect("Can allocate");
        assert_eq!(f.base.as_u64(), LARGE_PAGE_SIZE as u64);
        assert_eq!(f.size, LARGE_PAGE_SIZE);
        assert_eq!(f.affinity, 2);

        assert_eq!(tcache.free(), 0);

        let _f = tcache
            .allocate_base_page()
            .expect_err("Can't allocate more than we gave it");
    }
}
