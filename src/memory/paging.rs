use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

const PHYS_BASE: usize = 0x0040_0000;
const PHYS_END: usize = 0x0400_0000;
const PAGE_SIZE: usize = 4096;
const FRAME_COUNT: usize = (PHYS_END - PHYS_BASE) / PAGE_SIZE;

const BITMAP_WORDS: usize = (FRAME_COUNT + 31) / 32;
static BITMAP: [AtomicU32; BITMAP_WORDS] =
    unsafe { core::mem::transmute([0u32; BITMAP_WORDS]) };
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
    let word = idx / 32;
    let bit = idx % 32;
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
    let word = idx / 32;
    let bit = idx % 32;
    if word >= BITMAP_WORDS {
        return false;
    }
    (BITMAP[word].load(Ordering::Relaxed) >> bit) & 1 != 0
}

pub fn init() {
    for w in 0..BITMAP_WORDS {
        BITMAP[w].store(0xFFFF_FFFF, Ordering::Relaxed);
    }
    let remainder = FRAME_COUNT % 32;
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
        let word = i / 32;
        let bit = i % 32;
        let mask = 1u32 << bit;
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

/// Enable 32-bit paging with identity mapping for 0 .. PHYS_END.
/// Allocates page directory + page tables from the frame allocator.
/// Call once, after init(). Disables interrupts during the transition.
pub fn enable() {
    use crate::arch;

    const ENTRIES: usize = 1024;
    const PTE_P: u32 = 1;
    const PTE_W: u32 = 2;
    const PD_SIZE: usize = PHYS_END / (ENTRIES * PAGE_SIZE);

    let pd_phys = alloc_frame();
    if pd_phys == 0 {
        panic!("enable_paging: no frame for page directory");
    }

    arch::disable_interrupts();

    unsafe {
        let pd = pd_phys as *mut [u32; ENTRIES];
        core::ptr::write_volatile(pd, [0u32; ENTRIES]);

        for i in 0..PD_SIZE {
            let pt_phys = alloc_frame();
            if pt_phys == 0 {
                panic!("enable_paging: no frame for page table {}", i);
            }
            let pt = pt_phys as *mut [u32; ENTRIES];
            core::ptr::write_volatile(pt, [0u32; ENTRIES]);

            // Identity map: virtual addr == physical addr
            for j in 0..ENTRIES {
                let vaddr = (i * ENTRIES + j) * PAGE_SIZE;
                (*pt)[j] = vaddr as u32 | PTE_P | PTE_W;
            }

            (*pd)[i] = pt_phys as u32 | PTE_P | PTE_W;
        }
    }

    // Set CR3, enable PG bit
    unsafe {
        core::arch::asm!(
            "mov cr3, {pd}",
            "mov {tmp}, cr0",
            "or {tmp}, 0x80000000",
            "mov cr0, {tmp}",
            pd = in(reg) pd_phys as u32,
            tmp = out(reg) _,
            options(nostack),
        );
    }

    arch::enable_interrupts();
}
