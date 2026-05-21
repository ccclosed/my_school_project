use x86::io::outb;

use crate::arch;

use crate::keyboard;

const PIC1_CMD: u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
const PIC2_CMD: u16 = 0xA0;
const PIC2_DATA: u16 = 0xA1;
const PIC_EOI: u8 = 0x20;

pub fn init_pic() {
    unsafe {
        outb(PIC1_CMD, 0x11);
        outb(PIC1_DATA, 0x20);
        outb(PIC1_DATA, 0x04);
        outb(PIC1_DATA, 0x01);
        outb(PIC2_CMD, 0x11);
        outb(PIC2_DATA, 0x28);
        outb(PIC2_DATA, 0x02);
        outb(PIC2_DATA, 0x01);
        outb(PIC1_DATA, 0xFC); // Unmask IRQ0 (timer) and IRQ1 (keyboard)
        outb(PIC2_DATA, 0xEF); // Unmask IRQ12 (mouse) - bit 4 = 0
    }
}

fn eoi(irq: u8) {
    unsafe {
        if irq >= 8 {
            outb(PIC2_CMD, PIC_EOI);
        }
        outb(PIC1_CMD, PIC_EOI);
    }
}

/// Exception frame pushed by CPU on interrupt (without error code)
#[allow(dead_code)]
#[repr(C)]
struct ExceptionFrame {
    rip: u64,
    cs: u64,
    rflags: u64,
    rsp: u64,
    ss: u64,
}

#[no_mangle]
pub extern "C" fn divide_handler() {
    crate::serial::write_fmt(format_args!("[E] EXCEPTION: Divide by Zero\n"));
    arch::print_stack_trace();
    loop { arch::hlt(); }
}

#[no_mangle]
pub extern "C" fn invalid_opcode_handler() {
    crate::serial::write_fmt(format_args!("[E] EXCEPTION: Invalid Opcode\n"));
    arch::print_stack_trace();
    loop { arch::hlt(); }
}

#[no_mangle]
pub extern "C" fn gpf_handler(error_code: u64) {
    crate::serial::write_fmt(format_args!("[E] EXCEPTION: General Protection Fault (error: 0x{:x})\n", error_code));
    arch::print_stack_trace();
    loop { arch::hlt(); }
}

#[no_mangle]
pub extern "C" fn page_fault_handler(error_code: u64) {
    let cr2: u64;
    unsafe { core::arch::asm!("mov {}, cr2", out(reg) cr2); }
    crate::serial::write_fmt(format_args!(
        "[E] EXCEPTION: Page Fault at 0x{:016x} (error: 0x{:x})\n", 
        cr2, error_code
    ));
    arch::print_stack_trace();
    loop { arch::hlt(); }
}

#[no_mangle]
pub extern "C" fn double_fault_handler() {
    crate::serial::write_fmt(format_args!("[E] EXCEPTION: Double Fault\n"));
    loop { arch::hlt(); }
}

#[no_mangle]
pub extern "C" fn timer_handler(pushad_rsp: u64) -> u64 {
    crate::timer::tick();
    let new_rsp = crate::scheduler::schedule(pushad_rsp);
    eoi(0);
    new_rsp
}

#[no_mangle]
pub extern "C" fn keyboard_handler() {
    keyboard::handle_irq();
    eoi(1);
}

#[no_mangle]
pub extern "C" fn mouse_handler() {
    crate::mouse::handle_irq();
    eoi(12);
}
