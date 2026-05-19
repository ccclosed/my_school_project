use alloc::string::ToString;
use lazy_static::lazy_static;

#[repr(C)]
struct CpuidResult {
    eax: u32,
    ebx: u32,
    ecx: u32,
    edx: u32,
}

fn cpuid(leaf: u32) -> CpuidResult {
    let eax: u32;
    let ebx: u32;
    let ecx: u32;
    let edx: u32;
    unsafe {
        core::arch::asm!(
            "push ebx",
            "cpuid",
            "mov {ebx_out:e}, ebx",
            "pop ebx",
            inout("eax") leaf => eax,
            ebx_out = out(reg) ebx,
            out("ecx") ecx,
            out("edx") edx,
        );
    }
    CpuidResult { eax, ebx, ecx, edx }
}

lazy_static! {
    static ref VENDOR: alloc::string::String = {
        let r = cpuid(0);
        let mut bytes = [0u8; 13];
        bytes[0..4].copy_from_slice(&r.ebx.to_le_bytes());
        bytes[4..8].copy_from_slice(&r.edx.to_le_bytes());
        bytes[8..12].copy_from_slice(&r.ecx.to_le_bytes());
        let len = bytes.iter().position(|&b| b == 0).unwrap_or(12);
        core::str::from_utf8(&bytes[..len]).unwrap().to_string()
    };
}

pub fn vendor_id() -> &'static str {
    &VENDOR
}

pub fn brand_string() -> alloc::string::String {
    let mut brand = alloc::string::String::new();
    for leaf in 0x80000002..=0x80000004 {
        let r = cpuid(leaf);
        brand.push_str(&format_registers(r.eax, r.ebx, r.ecx, r.edx));
    }
    brand.trim().to_string()
}

fn format_registers(a: u32, b: u32, c: u32, d: u32) -> alloc::string::String {
    let mut s = alloc::string::String::with_capacity(16);
    for reg in [a, b, c, d] {
        for byte in reg.to_le_bytes() {
            if byte != 0 {
                s.push(byte as char);
            }
        }
    }
    s
}
