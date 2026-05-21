pub mod bucket_allocator;
pub mod paging;

use core::alloc::{GlobalAlloc, Layout};
use spin::Mutex;

use self::bucket_allocator::BucketAllocator;

pub const HEAP_SIZE: usize = 2 * 1024 * 1024;

#[repr(align(4096))]
#[allow(dead_code)]
struct HeapRegion([u8; HEAP_SIZE]);

static mut HEAP: HeapRegion = HeapRegion([0; HEAP_SIZE]);

struct KernelAllocator {
    inner: Mutex<BucketAllocator>,
}

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.inner
            .lock()
            .alloc(layout)
            .map(|p| p.as_ptr())
            .unwrap_or(core::ptr::null_mut())
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if let Some(ptr) = core::ptr::NonNull::new(ptr) {
            self.inner.lock().dealloc(ptr, layout);
        }
    }
}

#[global_allocator]
static ALLOCATOR: KernelAllocator = KernelAllocator {
    inner: Mutex::new(BucketAllocator::new()),
};

pub fn init_heap() {
    let heap_start = core::ptr::addr_of_mut!(HEAP).cast::<u8>();
    unsafe {
        ALLOCATOR
            .inner
            .lock()
            .init(heap_start, HEAP_SIZE);
    }
}

pub fn heap_stats() -> (usize, usize) {
    let alloc = ALLOCATOR.inner.lock();
    let used = alloc.used_bytes();
    let free = HEAP_SIZE.saturating_sub(used);
    (used, free)
}
