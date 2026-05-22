use core::sync::atomic::{AtomicU32, Ordering};
use x86::io::outb;

const PIT_CMD: u16 = 0x43;
const PIT_DATA: u16 = 0x40;
/// 1193182 Hz / 1193 ≈ 1000 Hz → tick = 1 ms
const PIT_DIVISOR: u16 = 1193;

/// Tick counter, incremented by the PIT IRQ0 handler (1000 Hz).
/// AtomicU32 suffices: 2^32 ticks at 1000 Hz = ~49.7 days before overflow.
static TICKS: AtomicU32 = AtomicU32::new(0);

/// Program PIT channel 0 to 1000 Hz (mode 3, square wave).
pub fn init() {
    unsafe {
        outb(PIT_CMD, 0x36); // channel 0, lo/hi access, mode 3, binary
        outb(PIT_DATA, (PIT_DIVISOR & 0xFF) as u8);
        outb(PIT_DATA, (PIT_DIVISOR >> 8) as u8);
    }
}

/// Called from the timer IRQ handler once per tick.
pub fn tick() {
    TICKS.fetch_add(1, Ordering::Relaxed);
}

/// Milliseconds since boot.
pub fn millis() -> u64 {
    let t = TICKS.load(Ordering::Relaxed);
    t as u64
}

/// Return (seconds, milliseconds) since boot.
pub fn elapsed() -> (u64, u64) {
    let ms = millis();
    (ms / 1000, ms % 1000)
}
