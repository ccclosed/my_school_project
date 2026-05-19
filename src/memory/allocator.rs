use core::alloc::Layout;
use core::mem;
use core::ptr::NonNull;

struct ListNode {
    size: usize,
    next: Option<NonNull<ListNode>>,
}

impl ListNode {
    const fn new(size: usize) -> Self {
        ListNode { size, next: None }
    }

    fn start_addr(&self) -> usize {
        self as *const Self as usize
    }

    fn end_addr(&self) -> usize {
        self.start_addr() + self.size
    }
}

pub struct LinkedListAllocator {
    head: ListNode,
    used: usize,
    heap_start: usize,
    heap_end: usize,
}

unsafe impl Send for LinkedListAllocator {}
unsafe impl Sync for LinkedListAllocator {}

impl LinkedListAllocator {
    pub const fn new() -> Self {
        LinkedListAllocator { 
            head: ListNode::new(0), 
            used: 0,
            heap_start: 0,
            heap_end: 0,
        }
    }

    pub unsafe fn init(&mut self, heap_start: *mut u8, heap_size: usize) {
        self.heap_start = heap_start as usize;
        self.heap_end = self.heap_start + heap_size;
        
        let node = heap_start as *mut ListNode;
        (*node).size = heap_size;
        (*node).next = None;
        self.head.next = NonNull::new(node);
    }

    pub fn used_bytes(&self) -> usize {
        self.used
    }

    pub fn alloc(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        let size = layout.size().max(8);
        let align = layout.align().max(8);

        let mut cursor: *mut Option<NonNull<ListNode>> = &mut self.head.next;
        unsafe {
            while let Some(mut node) = *cursor {
                let region = node.as_mut();
                let region_start = region.start_addr();
                let alloc_start = align_up(region_start, align);
                let alloc_end = alloc_start.checked_add(size)?;

                if alloc_end <= region.end_addr() {
                    self.used += size;

                    let excess = region.end_addr() - alloc_end;
                    if excess >= mem::size_of::<ListNode>() {
                        let leftover = alloc_end as *mut ListNode;
                        (*leftover).size = excess;

                        if alloc_start == region_start {
                            // Replace current node with leftover
                            (*leftover).next = region.next.take();
                            *cursor = NonNull::new(leftover);
                        } else {
                            // Shrink current region, leftover follows
                            region.size = alloc_start - region_start;
                            (*leftover).next = region.next.take();
                            region.next = NonNull::new(leftover);
                        }
                    } else {
                        // No usable leftover — remove this node
                        *cursor = region.next.take();
                    }

                    return NonNull::new(alloc_start as *mut u8);
                }

                cursor = &mut region.next;
            }
        }
        None
    }

    /// Insert a block into the sorted free list, merging with adjacent blocks.
    pub unsafe fn dealloc(&mut self, ptr: NonNull<u8>, layout: Layout) {
        let size = layout.size().max(8);
        let align = layout.align().max(8);
        self.used = self.used.saturating_sub(size);

        let addr = ptr.as_ptr() as usize;
        
        // Validate address is within heap bounds
        if addr < self.heap_start || addr.checked_add(size).map_or(true, |end| end > self.heap_end) {
            return;
        }
        
        // Validate alignment
        if addr % align != 0 {
            return;
        }
        
        // Ensure size is large enough for a ListNode
        if size < mem::size_of::<ListNode>() {
            return;
        }

        let freed = addr as *mut ListNode;
        (*freed).size = size;

        // Walk the sorted free list to find insertion point
        let mut prev: Option<NonNull<ListNode>> = None;
        let mut curr: Option<NonNull<ListNode>> = self.head.next;

        loop {
            let curr_addr = match curr {
                Some(c) => c.as_ref().start_addr(),
                None => usize::MAX,
            };

            if addr < curr_addr {
                // Insert between prev and curr
                let end = addr + size;

                // Try to merge with prev
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

                // Try to merge with curr
                if let Some(mut c) = curr {
                    if end == curr_addr {
                        let curr_next = c.as_mut().next.take();
                        let curr_size = c.as_mut().size;
                        (*freed).size = size + curr_size;
                        (*freed).next = curr_next;
                        if let Some(mut p) = prev {
                            p.as_mut().next = NonNull::new(freed);
                        } else {
                            self.head.next = NonNull::new(freed);
                        }
                        return;
                    }
                }

                // No merge — insert as new node
                (*freed).size = size;
                (*freed).next = curr;
                if let Some(mut p) = prev {
                    p.as_mut().next = NonNull::new(freed);
                } else {
                    self.head.next = NonNull::new(freed);
                }
                return;
            }

            if addr == curr_addr {
                // Double free detected - ignore
                return;
            }

            prev = curr;
            curr = curr.and_then(|c| c.as_ref().next);
        }
    }
}

pub fn dump_free_list() {
    crate::memory::bucket_allocator::dump_stats();
}

fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}
