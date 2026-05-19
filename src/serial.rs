/// Minimal 16550 UART (COM1) driver for QEMU serial output.
use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};
use x86::io::{inb, outb};

const COM1: u16 = 0x3F8;

static INIT: AtomicBool = AtomicBool::new(false);

/// Returns true if serial port is ready for output.
#[allow(dead_code)]
pub fn is_initialized() -> bool {
    INIT.load(Ordering::Relaxed)
}

/// Initialize COM1 at 115200 baud, 8N1.
pub fn init() {
    unsafe {
        outb(COM1 + 1, 0x00);  // Disable interrupts
        outb(COM1 + 3, 0x80);  // DLAB on
        outb(COM1 + 0, 0x01);  // Divisor low (115200)
        outb(COM1 + 1, 0x00);  // Divisor high
        outb(COM1 + 3, 0x03);  // 8N1
        outb(COM1 + 2, 0xC7);  // FIFO: enable, clear, 14-byte threshold
        outb(COM1 + 4, 0x0B);  // DTR, RTS, aux out 2
    }
    INIT.store(true, Ordering::Release);
}

/// Write a single byte to COM1.
fn putb(byte: u8) {
    unsafe {
        while inb(COM1 + 5) & 0x20 == 0 {
            core::hint::spin_loop();
        }
        outb(COM1, byte);
    }
}

/// Read a byte from COM1 if available (non-blocking).
pub fn poll_char() -> Option<u8> {
    if !INIT.load(Ordering::Acquire) {
        return None;
    }
    unsafe {
        if inb(COM1 + 5) & 0x01 != 0 {
            Some(inb(COM1))
        } else {
            None
        }
    }
}

/// Write a string to COM1. No-op if serial not initialized.
pub fn write(s: &str) {
    if !INIT.load(Ordering::Acquire) {
        return;
    }
    for &b in s.as_bytes() {
        putb(b);
    }
}

struct SerialWriter;

impl fmt::Write for SerialWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        write(s);
        Ok(())
    }
}

/// Write a formatted message to COM1. No-op if serial not initialized.
pub fn write_fmt(args: fmt::Arguments) {
    use fmt::Write;
    let _ = SerialWriter.write_fmt(args);
}
