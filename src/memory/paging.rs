use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const PHYS_BASE: usize = 0x0040_0000;
const PHYS_END: usize = 0x4000_0000; // 1GB
const PAGE_SIZE: usize = 4096;
const FRAME_COUNT: usize = (PHYS_END - PHYS_BASE) / PAGE_SIZE;

const BITMAP_WORDS: usize = (FRAME_COUNT + 63) / 64;
static BITMAP: [AtomicU64; BITMAP_WORDS] =
    unsafe { core::mem::transmute([0u64; BITMAP_WORDS]) };
static INITIALIZED: AtomicBool = AtomicBool::new(false);

fn page_index(phys: usize) -> Option<usize> {
    if phys < PHYS_BASE || phys >= PHYS_END {
        return None;
    }
    Some((phys - PHYS_BASE) / PAGE_SIZE)
}

fn set_bit(idx: usize, free: bool) {
    if idx >= FRAME_COUNT {
        return;
    }
    let word = idx / 64;
    let bit = idx % 64;
    if word >= BITMAP_WORDS {
        return;
    }
    if free {
        BITMAP[word].fetch_or(1 << bit, Ordering::Relaxed);
    } else {
        BITMAP[word].fetch_and(!(1 << bit), Ordering::Relaxed);
    }
}

fn test_bit(idx: usize) -> bool {
    if idx >= FRAME_COUNT {
        return false;
    }
    let word = idx / 64;
    let bit = idx % 64;
    if word >= BITMAP_WORDS {
        return false;
    }
    (BITMAP[word].load(Ordering::Relaxed) >> bit) & 1 != 0
}

pub fn init() {
    for w in 0..BITMAP_WORDS {
        BITMAP[w].store(0xFFFF_FFFF_FFFF_FFFF, Ordering::Relaxed);
    }
    let remainder = FRAME_COUNT % 64;
    if remainder > 0 && BITMAP_WORDS > 0 {
        BITMAP[BITMAP_WORDS - 1]
            .fetch_and((1 << remainder) - 1, Ordering::Relaxed);
    }
    let kernel_start = 0x0010_0000;
    let kernel_end = PHYS_BASE;
    let mut addr = kernel_start;
    while addr < kernel_end {
        mark_used(addr);
        addr = addr.saturating_add(PAGE_SIZE);
        if addr < kernel_start {
            break; // Overflow protection
        }
    }
    INITIALIZED.store(true, Ordering::Release);
}

pub fn mark_used(phys: usize) {
    if let Some(idx) = page_index(phys) {
        set_bit(idx, false);
    }
}

#[allow(dead_code)]
pub fn mark_free(phys: usize) {
    if let Some(idx) = page_index(phys) {
        set_bit(idx, true);
    }
}

#[allow(dead_code)]
pub fn alloc_frame() -> usize {
    if !INITIALIZED.load(Ordering::Acquire) {
        return 0;
    }
    for i in 0..FRAME_COUNT {
        let word = i / 64;
        let bit = i % 64;
        let mask = 1u64 << bit;
        let prev = BITMAP[word].fetch_and(!mask, Ordering::AcqRel);
        if prev & mask != 0 {
            return PHYS_BASE + i * PAGE_SIZE;
        }
    }
    0
}

#[allow(dead_code)]
pub fn free_frame(phys: usize) {
    if phys % PAGE_SIZE != 0 {
        return;
    }
    if let Some(idx) = page_index(phys) {
        set_bit(idx, true);
    }
}

pub fn stats() -> (usize, usize, usize) {
    let mut free = 0;
    for i in 0..FRAME_COUNT {
        if test_bit(i) {
            free += 1;
        }
    }
    (FRAME_COUNT, free, FRAME_COUNT - free)
}

/// Enable 64-bit paging (already enabled by bootloader, this is a no-op for now).
/// In x86_64, paging is enabled during the transition to long mode in asm.rs.
/// This function exists for API compatibility but doesn't need to do anything.
pub fn enable() {
    // Paging is already enabled by the bootloader in asm.rs
    // We're using 2MB pages for the first 1GB (identity mapped)
}
