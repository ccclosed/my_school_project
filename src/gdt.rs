use core::mem::size_of;

const ACCESS_CODE: u8 = 0x9A;
const ACCESS_DATA: u8 = 0x92;
const GRAN_4K_64: u8 = 0xAF; // 64-bit code segment
const GRAN_DATA: u8 = 0xCF;

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

/// TSS descriptor (16 bytes for x86_64) — takes two GDT entries.
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct TssDescriptor {
    limit_low: u16,
    base_low: u16,
    base_middle: u8,
    access: u8,
    granularity: u8,
    base_high: u8,
    base_upper: u32,
    reserved: u32,
}

impl TssDescriptor {
    fn from_tss(tss: &TaskStateSegment) -> Self {
        let base = tss as *const _ as u64;
        let limit = (size_of::<TaskStateSegment>() - 1) as u16;
        Self {
            limit_low: limit,
            base_low: base as u16,
            base_middle: (base >> 16) as u8,
            access: 0x89,
            granularity: 0x00,
            base_high: (base >> 24) as u8,
            base_upper: (base >> 32) as u32,
            reserved: 0,
        }
    }
}

/// x86_64 Task State Segment (104 bytes minimum).
#[repr(C, packed)]
struct TaskStateSegment {
    _reserved0: u32,
    rsp0: u64,
    rsp1: u64,
    rsp2: u64,
    _reserved1: u64,
    ist1: u64,
    ist2: u64,
    ist3: u64,
    ist4: u64,
    ist5: u64,
    ist6: u64,
    ist7: u64,
    _reserved2: u64,
    _reserved3: u16,
    io_map_base: u16,
}

#[repr(C, packed)]
struct GdtPointer {
    limit: u16,
    base: u64,
}

/// Full GDT: null, code, data, user code, user data, TSS descriptor (16 bytes).
#[repr(C, packed)]
struct GdtStruct {
    entries: [GdtEntry; 5],
    tss: TssDescriptor,
}

static mut GDT: GdtStruct = GdtStruct {
    entries: [
        GdtEntry::null(),
        GdtEntry::new(0, 0, ACCESS_CODE, GRAN_4K_64),  // Kernel code (64-bit)
        GdtEntry::new(0, 0, ACCESS_DATA, GRAN_DATA),   // Kernel data
        GdtEntry::new(0, 0, 0xFA, GRAN_4K_64),         // User code (ring 3)
        GdtEntry::new(0, 0, 0xF2, GRAN_DATA),          // User data (ring 3)
    ],
    // TSS descriptor filled at runtime
    tss: TssDescriptor { 
        limit_low: 0, base_low: 0, base_middle: 0, access: 0, 
        granularity: 0, base_high: 0, base_upper: 0, reserved: 0 
    },
};

pub const KERNEL_CODE_SELECTOR: u16 = 0x08;
#[allow(dead_code)]
pub const KERNEL_DATA_SELECTOR: u16 = 0x10;
#[allow(dead_code)]
const TSS_SELECTOR: u16 = 0x28;

// Reference to the dedicated double-fault stack defined in asm.rs.
extern "C" {
    static df_stack_top: u8;
}

static mut TSS: TaskStateSegment = TaskStateSegment {
    _reserved0: 0,
    rsp0: 0,
    rsp1: 0,
    rsp2: 0,
    _reserved1: 0,
    ist1: 0,
    ist2: 0,
    ist3: 0,
    ist4: 0,
    ist5: 0,
    ist6: 0,
    ist7: 0,
    _reserved2: 0,
    _reserved3: 0,
    io_map_base: size_of::<TaskStateSegment>() as u16,
};

extern "C" {
    fn load_gdt(ptr: *const GdtPointer);
    fn reload_segments();
    fn load_tss();
}

pub fn init() {
    unsafe {
        TSS.rsp0 = (&df_stack_top as *const u8) as u64;
        TSS.ist1 = (&df_stack_top as *const u8) as u64;

        let tss_ref: &TaskStateSegment = &*core::ptr::addr_of!(TSS);
        GDT.tss = TssDescriptor::from_tss(tss_ref);
    }

    let ptr = GdtPointer {
        limit: (size_of::<GdtStruct>() - 1) as u16,
        base: core::ptr::addr_of_mut!(GDT) as u64,
    };
    unsafe {
        load_gdt(&ptr);
        reload_segments();
        load_tss();
    }
}
