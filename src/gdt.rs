use core::mem::size_of;

const ACCESS_CODE: u8 = 0x9A;
const ACCESS_DATA: u8 = 0x92;
const GRAN_4K_32: u8 = 0xCF;

#[repr(C)]
#[derive(Clone, Copy)]
struct GdtEntry {
    limit_low: u16,
    base_low: u16,
    base_middle: u8,
    access: u8,
    granularity: u8,
    base_high: u8,
}

impl GdtEntry {
    const fn null() -> Self {
        Self { limit_low: 0, base_low: 0, base_middle: 0, access: 0, granularity: 0, base_high: 0 }
    }

    const fn new(base: u32, limit: u32, access: u8, granularity: u8) -> Self {
        Self {
            limit_low: limit as u16,
            base_low: base as u16,
            base_middle: (base >> 16) as u8,
            access,
            granularity,
            base_high: (base >> 24) as u8,
        }
    }
}

/// TSS descriptor (8 bytes) — unlike a regular segment descriptor.
/// For a 32-bit TSS, the base is 32-bit and the limit is 20 bits (max 104 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
struct TssDescriptor {
    limit_low: u16,
    base_low: u16,
    base_middle: u8,
    access: u8,
    granularity: u8,
    base_high: u8,
}

impl TssDescriptor {
    fn from_tss(tss: &TaskStateSegment) -> Self {
        let base = tss as *const _ as u32;
        let limit = (size_of::<TaskStateSegment>() - 1) as u16;
        Self {
            limit_low: limit,
            base_low: base as u16,
            base_middle: (base >> 16) as u8,
            access: 0x89,
            granularity: 0x00,
            base_high: (base >> 24) as u8,
        }
    }
}

/// i686 Task State Segment (104 bytes, Intel Vol3 Figure 7-2).
#[repr(C, packed)]
struct TaskStateSegment {
    prev: u16,
    _reserved0: u16,
    esp0: u32,
    ss0: u16,
    _reserved1: u16,
    esp1: u32,
    ss1: u16,
    _reserved2: u16,
    esp2: u32,
    ss2: u16,
    _reserved3: u16,
    cr3: u32,
    eip: u32,
    eflags: u32,
    eax: u32,
    ecx: u32,
    edx: u32,
    ebx: u32,
    esp: u32,
    ebp: u32,
    esi: u32,
    edi: u32,
    es: u16,
    _reserved4: u16,
    cs: u16,
    _reserved5: u16,
    ss: u16,
    _reserved6: u16,
    ds: u16,
    _reserved7: u16,
    fs: u16,
    _reserved8: u16,
    gs: u16,
    _reserved9: u16,
    ldt: u16,
    _reserved10: u16,
    _reserved11: u16,
    io_map_base: u16,
}

#[repr(C, packed)]
struct GdtPointer {
    limit: u16,
    base: u32,
}

/// Full GDT: null, code, data, TSS descriptor.
#[repr(C, packed)]
struct GdtStruct {
    entries: [GdtEntry; 3],
    tss: TssDescriptor,
}

static mut GDT: GdtStruct = GdtStruct {
    entries: [
        GdtEntry::null(),
        GdtEntry::new(0, 0xFFFFF, ACCESS_CODE, GRAN_4K_32),
        GdtEntry::new(0, 0xFFFFF, ACCESS_DATA, GRAN_4K_32),
    ],
    // TSS descriptor filled at runtime via init_tss()
    tss: TssDescriptor { limit_low: 0, base_low: 0, base_middle: 0, access: 0, granularity: 0, base_high: 0 },
};

pub const KERNEL_CODE_SELECTOR: u16 = 0x08;
pub const KERNEL_DATA_SELECTOR: u16 = 0x10;
#[allow(dead_code)]
const TSS_SELECTOR: u16 = 0x18;

// Reference to the dedicated double-fault stack defined in asm.rs.
extern "C" {
    static df_stack_top: u8;
}

static mut TSS: TaskStateSegment = TaskStateSegment {
    prev: 0, _reserved0: 0, esp0: 0, ss0: KERNEL_DATA_SELECTOR,
    _reserved1: 0, esp1: 0, ss1: 0, _reserved2: 0,
    esp2: 0, ss2: 0, _reserved3: 0,
    cr3: 0, eip: 0, eflags: 0, eax: 0, ecx: 0, edx: 0, ebx: 0,
    esp: 0, ebp: 0, esi: 0, edi: 0,
    es: 0, _reserved4: 0, cs: 0, _reserved5: 0, ss: 0, _reserved6: 0,
    ds: 0, _reserved7: 0, fs: 0, _reserved8: 0, gs: 0, _reserved9: 0,
    ldt: 0, _reserved10: 0, _reserved11: 0,
    io_map_base: size_of::<TaskStateSegment>() as u16,
};

extern "C" {
    fn load_gdt(ptr: *const GdtPointer);
    fn reload_segments();
    fn load_tss();
}

pub fn init() {
    unsafe {
        TSS.esp0 = (&df_stack_top as *const u8) as u32;
        TSS.ss0 = KERNEL_DATA_SELECTOR;

        let tss_ref: &TaskStateSegment = &*core::ptr::addr_of!(TSS);
        GDT.tss = TssDescriptor::from_tss(tss_ref);
    }

    let ptr = GdtPointer {
        limit: (size_of::<GdtStruct>() - 1) as u16,
        base: unsafe { core::ptr::addr_of!(GDT) as u32 },
    };
    unsafe {
        load_gdt(&ptr);
        reload_segments();
        load_tss();
    }
}
