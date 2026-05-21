#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

#[macro_use]
mod vga;

mod arch;
mod asm;
mod cpu;
mod drivers;
mod fs;
mod gdt;
mod idt;
mod interrupts;
mod keyboard;
mod log;
mod mouse;
mod rtc;
mod memory;
mod scheduler;
mod serial;
mod shell;
mod timer;

use core::panic::PanicInfo;
use crate::vga::Color;

#[no_mangle]
pub extern "C" fn kernel_main() -> ! {
    gdt::init();
    idt::init();
    interrupts::init_pic();
    timer::init();

    vga::init();
    serial::init();
    memory::init_heap();
    memory::paging::init();
    memory::paging::enable();

    println!("Rust Kernel (x86_64) - boot OK");
    println!("Heap: {} KiB", memory::HEAP_SIZE / 1024);

    // Quick heap sanity check before heavier subsystems.
    {
        let mut v = alloc::vec::Vec::new();
        v.push(42u8);
        println!("Heap alloc test: OK ({})", v[0]);
    }

    drivers::net::init();
    println!("Network init: OK");

    // Auto-configure network via DHCP if NIC is available
    let net_status = drivers::net::status();
    if net_status.kind != drivers::net::NicKind::None {
        println!("Requesting IP via DHCP...");
        let xid = timer::millis() as u32;
        match drivers::net::dhcp::dhcp_request(&net_status.mac, xid) {
            Some(cfg) => {
                drivers::net::set_config(cfg);
                println!("DHCP: IP {}.{}.{}.{}", cfg.ip[0], cfg.ip[1], cfg.ip[2], cfg.ip[3]);
                println!("      Gateway {}.{}.{}.{}", cfg.gateway[0], cfg.gateway[1], cfg.gateway[2], cfg.gateway[3]);
            }
            None => {
                println!("DHCP: timeout, using fallback IP 10.0.2.15");
                drivers::net::set_config(drivers::net::NetConfig {
                    ip: [10, 0, 2, 15],
                    subnet: [255, 255, 255, 0],
                    gateway: [10, 0, 2, 2],
                    dns: [8, 8, 8, 8],
                });
            }
        }
    }

    scheduler::init();

    arch::enable_interrupts();
    println!("Type 'help' for commands.");

    shell::run();
}

fn panic_banner() {
    vga::set_color(Color::White, Color::Red);
    vga::clear_screen();
    vga::write_at(2, 30, "KERNEL PANIC");
    vga::set_color(Color::LightGray, Color::Black);
}

#[alloc_error_handler]
fn alloc_error(layout: core::alloc::Layout) -> ! {
    let h = crate::memory::heap_stats();
    println!("ALLOC FAILED: size={} align={} heap: {}/{}",
        layout.size(), layout.align(), h.0, crate::memory::HEAP_SIZE);
    crate::memory::bucket_allocator::dump_stats();
    arch::print_stack_trace();

    panic_banner();
    vga::write_fmt(format_args!("\nAllocation failed!\n"));
    vga::write_fmt(format_args!("Size: {} bytes, Align: {}\n", layout.size(), layout.align()));
    vga::write_fmt(format_args!("Heap: {}/{} bytes used\n", h.0, crate::memory::HEAP_SIZE));
    loop { arch::hlt(); }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("KERNEL PANIC: {}", info);
    if let Some(loc) = info.location() {
        println!("  at {}:{}", loc.file(), loc.line());
    }
    arch::print_stack_trace();

    panic_banner();
    vga::write_fmt(format_args!("\n{}\n", info));
    if let Some(loc) = info.location() {
        vga::write_fmt(format_args!("at {}:{}\n", loc.file(), loc.line()));
    }
    loop { arch::hlt(); }
}
