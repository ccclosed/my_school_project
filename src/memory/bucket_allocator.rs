/// Bucket allocator inspired by Windows LFH
/// Fast O(1) allocation for common sizes, fallback to linked list for large blocks

use core::alloc::Layout;
use core::ptr::NonNull;

/// Bucket sizes: 8, 16, 32, 64, 128, 256, 512, 1024, 2048 bytes
const BUCKET_SIZES: [usize; 9] = [8, 16, 32, 64, 128, 256, 512, 1024, 2048];
const BUCKET_COUNT: usize = BUCKET_SIZES.len();
const BLOCKS_PER_BUCKET: usize = 256; // 256 blocks per bucket

/// Free list node for bucket allocator
#[repr(C)]
struct FreeNode {
    next: Option<NonNull<FreeNode>>,
}

/// Bucket for fixed-size allocations
struct Bucket {
    block_size: usize,
    free_list: Option<NonNull<FreeNode>>,
    total_blocks: usize,
    free_blocks: usize,
}

impl Bucket {
    const fn new(block_size: usize) -> Self {
        Self {
            block_size,
            free_list: None,
            total_blocks: 0,
            free_blocks: 0,
        }
    }

    /// Initialize bucket with memory region
    unsafe fn init(&mut self, start: *mut u8, size: usize) {
        let block_count = size / self.block_size;
        self.total_blocks = block_count;
        self.free_blocks = block_count;

        // Build free list
        let mut current = start;
        for i in 0..block_count {
            let node = current as *mut FreeNode;
            if i < block_count - 1 {
                (*node).next = NonNull::new(current.add(self.block_size) as *mut FreeNode);
            } else {
                (*node).next = None;
            }
            current = current.add(self.block_size);
        }
        self.free_list = NonNull::new(start as *mut FreeNode);
    }

    /// Allocate a block from this bucket
    fn alloc(&mut self) -> Option<NonNull<u8>> {
        if let Some(mut node) = self.free_list {
            unsafe {
                self.free_list = node.as_mut().next;
                self.free_blocks -= 1;
                Some(node.cast())
            }
        } else {
            None
        }
    }

    /// Free a block back to this bucket
    unsafe fn dealloc(&mut self, ptr: NonNull<u8>) {
        let node = ptr.cast::<FreeNode>().as_ptr();
        // No double-free detection for buckets - trust the caller
        // (checking would be O(n) and defeat the purpose of O(1) buckets)
        (*node).next = self.free_list;
        self.free_list = NonNull::new(node);
        self.free_blocks += 1;
    }
}

/// Large block allocator (linked list for >2KB)
struct LargeNode {
    size: usize,
    next: Option<NonNull<LargeNode>>,
}

impl LargeNode {
    fn start_addr(&self) -> usize {
        self as *const Self as usize
    }

    fn end_addr(&self) -> usize {
        self.start_addr() + self.size
    }
}

pub struct BucketAllocator {
    buckets: [Bucket; BUCKET_COUNT],
    large_head: LargeNode,
    used: usize,
    heap_start: usize,
    heap_end: usize,
}

unsafe impl Send for BucketAllocator {}
unsafe impl Sync for BucketAllocator {}

impl BucketAllocator {
    pub const fn new() -> Self {
        const BUCKET_INIT: Bucket = Bucket::new(0);
        Self {
            buckets: [BUCKET_INIT; BUCKET_COUNT],
            large_head: LargeNode { size: 0, next: None },
            used: 0,
            heap_start: 0,
            heap_end: 0,
        }
    }

    pub unsafe fn init(&mut self, heap_start: *mut u8, heap_size: usize) {
        self.heap_start = heap_start as usize;
        self.heap_end = self.heap_start + heap_size;

        let mut current = heap_start;
        let mut remaining = heap_size;

        // Allocate memory for each bucket
        for i in 0..BUCKET_COUNT {
            let block_size = BUCKET_SIZES[i];
            self.buckets[i].block_size = block_size;
            
            let bucket_size = block_size * BLOCKS_PER_BUCKET;
            if remaining < bucket_size {
                break;
            }

            self.buckets[i].init(current, bucket_size);
            current = current.add(bucket_size);
            remaining -= bucket_size;
        }

        // Rest goes to large allocator
        if remaining > core::mem::size_of::<LargeNode>() {
            let node = current as *mut LargeNode;
            (*node).size = remaining;
            (*node).next = None;
            self.large_head.next = NonNull::new(node);
        }
    }

    pub fn used_bytes(&self) -> usize {
        self.used
    }

    /// Find bucket index for given size
    fn bucket_index(&self, size: usize) -> Option<usize> {
        for (i, &bucket_size) in BUCKET_SIZES.iter().enumerate() {
            if size <= bucket_size {
                return Some(i);
            }
        }
        None
    }

    pub fn alloc(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        let size = layout.size().max(8);
        let align = layout.align().max(8);

        // Try bucket allocator for small sizes
        // But only if the bucket size satisfies the alignment requirement
        if let Some(idx) = self.bucket_index(size) {
            let bucket_size = self.buckets[idx].block_size;
            // Bucket allocations are naturally aligned to their size (power of 2)
            // Only use bucket if alignment requirement is satisfied
            if bucket_size >= align {
                if let Some(ptr) = self.buckets[idx].alloc() {
                    self.used += bucket_size;
                    return Some(ptr);
                }
            }
        }

        // Fallback to large allocator
        self.alloc_large(size, align)
    }

    fn alloc_large(&mut self, size: usize, align: usize) -> Option<NonNull<u8>> {
        let mut cursor: *mut Option<NonNull<LargeNode>> = &mut self.large_head.next;
        
        unsafe {
            while let Some(mut node) = *cursor {
                let region = node.as_mut();
                let region_start = region.start_addr();
                let alloc_start = align_up(region_start, align);
                let alloc_end = alloc_start.checked_add(size)?;

                if alloc_end <= region.end_addr() {
                    self.used += size;

                    let excess = region.end_addr() - alloc_end;
                    if excess >= core::mem::size_of::<LargeNode>() {
                        let leftover = alloc_end as *mut LargeNode;
                        (*leftover).size = excess;

                        if alloc_start == region_start {
                            (*leftover).next = region.next.take();
                            *cursor = NonNull::new(leftover);
                        } else {
                            region.size = alloc_start - region_start;
                            (*leftover).next = region.next.take();
                            region.next = NonNull::new(leftover);
                        }
                    } else {
                        *cursor = region.next.take();
                    }

                    return NonNull::new(alloc_start as *mut u8);
                }

                cursor = &mut region.next;
            }
        }
        None
    }

    pub unsafe fn dealloc(&mut self, ptr: NonNull<u8>, layout: Layout) {
        let size = layout.size().max(8);

        // Check if it's from a bucket
        if let Some(idx) = self.bucket_index(size) {
            let bucket_size = self.buckets[idx].block_size;
            self.used = self.used.saturating_sub(bucket_size);
            self.buckets[idx].dealloc(ptr);
            return;
        }

        // Large block dealloc
        self.dealloc_large(ptr, size);
    }

    unsafe fn dealloc_large(&mut self, ptr: NonNull<u8>, size: usize) {
        let addr = ptr.as_ptr() as usize;
        
        if addr < self.heap_start || addr.checked_add(size).map_or(true, |end| end > self.heap_end) {
            return;
        }

        self.used = self.used.saturating_sub(size);

        let freed = addr as *mut LargeNode;
        (*freed).size = size;

        let mut prev: Option<NonNull<LargeNode>> = None;
        let mut curr: Option<NonNull<LargeNode>> = self.large_head.next;

        loop {
            let curr_addr = match curr {
                Some(c) => c.as_ref().start_addr(),
                None => usize::MAX,
            };

            if addr < curr_addr {
                let end = addr + size;

                // Merge with prev
                if let Some(mut p) = prev {
                    let prev_end = p.as_ref().start_addr() + p.as_ref().size;
                    if prev_end == addr {
                        p.as_mut().size += size;
                        if let Some(mut c) = curr {
                            let p_end = p.as_ref().end_addr();
                            if p_end == c.as_ref().start_addr() {
                                p.as_mut().size += c.as_ref().size;
                                p.as_mut().next = c.as_mut().next.take();
                            }
                        }
                        return;
                    }
                }

                // Merge with curr
                if let Some(mut c) = curr {
                    if end == curr_addr {
                        let curr_next = c.as_mut().next.take();
                        let curr_size = c.as_mut().size;
                        (*freed).size = size + curr_size;
                        (*freed).next = curr_next;
                        if let Some(mut p) = prev {
                            p.as_mut().next = NonNull::new(freed);
                        } else {
                            self.large_head.next = NonNull::new(freed);
                        }
                        return;
                    }
                }

                // No merge
                (*freed).next = curr;
                if let Some(mut p) = prev {
                    p.as_mut().next = NonNull::new(freed);
                } else {
                    self.large_head.next = NonNull::new(freed);
                }
                return;
            }

            if addr == curr_addr {
                return; // Double free
            }

            prev = curr;
            curr = curr.and_then(|c| c.as_ref().next);
        }
    }
}

fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}

pub fn dump_stats() {
    let alloc = crate::memory::ALLOCATOR.inner.lock();
    crate::println!("Bucket allocator stats:");
    for i in 0..BUCKET_COUNT {
        let bucket = &alloc.buckets[i];
        if bucket.total_blocks > 0 {
            let used = bucket.total_blocks - bucket.free_blocks;
            crate::println!("  {}B: {}/{} blocks used", 
                bucket.block_size, used, bucket.total_blocks);
        }
    }
}
