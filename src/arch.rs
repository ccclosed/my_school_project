//! CPU control instructions (i686 inline asm).

/// Walk the frame pointer (EBP) chain and print return addresses.
/// Requires `-C force-frame-pointers=yes` or functions compiled without
/// omit-frame-pointer.
pub fn print_stack_trace() {
    let ebp: u32;
    unsafe {
        core::arch::asm!("mov {}, ebp", out(reg) ebp);
    }
    crate::serial::write_fmt(format_args!("--- Stack trace ---\n"));
    let mut frame = ebp;
    for i in 0..16 {
        if frame == 0 {
            break;
        }
        // Reject obviously invalid addresses (below 1 MiB or above 64 MiB)
        // Also check alignment (must be 4-byte aligned)
        if frame < 0x100000 || frame > 0x0400_0000 || frame % 4 != 0 {
            crate::serial::write_fmt(format_args!("  #{:02}  <invalid frame 0x{:08x}>\n", i, frame));
            break;
        }
        
        // Validate that we can safely read from this address
        let frame_ptr = frame as *const u32;
        let next_frame_ptr = frame_ptr as usize;
        let ret_addr_ptr = unsafe { frame_ptr.add(1) } as usize;
        
        // Check both pointers are in valid range before dereferencing
        if next_frame_ptr < 0x100000 || next_frame_ptr > 0x0400_0000 ||
           ret_addr_ptr < 0x100000 || ret_addr_ptr > 0x0400_0000 {
            crate::serial::write_fmt(format_args!("  #{:02}  <invalid pointer>\n", i));
            break;
        }
        
        unsafe {
            let ret_addr = *frame_ptr.add(1);
            crate::serial::write_fmt(format_args!("  #{:02}  0x{:08x}\n", i, ret_addr));
            frame = *frame_ptr;
        }
    }
    crate::serial::write_fmt(format_args!("-------------------\n"));
}

pub fn hlt() {
    unsafe {
        core::arch::asm!("hlt", options(nomem, nostack, preserves_flags));
    }
}

pub fn enable_interrupts() {
    unsafe {
        core::arch::asm!("sti", options(nomem, nostack));
    }
}

#[allow(dead_code)]
pub fn disable_interrupts() {
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack));
    }
}
