#![allow(dead_code)]

/// PS/2 Mouse driver with scroll wheel support
use core::sync::atomic::{AtomicBool, Ordering};
use lazy_static::lazy_static;
use spin::Mutex;
use x86::io::{inb, outb};

const PS2_DATA: u16 = 0x60;
const PS2_STATUS: u16 = 0x64;
const PS2_COMMAND: u16 = 0x64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseEvent {
    Move { dx: i16, dy: i16 },
    Scroll { delta: i8 },
    LeftClick,
    RightClick,
    MiddleClick,
}

struct MouseState {
    cycle: u8,
    bytes: [u8; 4],
    has_scroll: bool,
}

impl MouseState {
    const fn new() -> Self {
        Self {
            cycle: 0,
            bytes: [0; 4],
            has_scroll: false,
        }
    }
}

lazy_static! {
    static ref MOUSE_STATE: Mutex<MouseState> = Mutex::new(MouseState::new());
    static ref EVENT_QUEUE: Mutex<EventRing> = Mutex::new(EventRing::new());
}

static INITIALIZED: AtomicBool = AtomicBool::new(false);

struct EventRing {
    buf: [Option<MouseEvent>; 32],
    head: usize,
    tail: usize,
}

impl EventRing {
    const fn new() -> Self {
        Self {
            buf: [None; 32],
            head: 0,
            tail: 0,
        }
    }

    fn push(&mut self, ev: MouseEvent) -> bool {
        let next = (self.head + 1) % 32;
        if next == self.tail {
            return false;
        }
        self.buf[self.head] = Some(ev);
        self.head = next;
        true
    }

    fn pop(&mut self) -> Option<MouseEvent> {
        if self.tail == self.head {
            return None;
        }
        let ev = self.buf[self.tail].take();
        self.tail = (self.tail + 1) % 32;
        ev
    }
}

fn wait_write() {
    for _ in 0..10000 {
        unsafe {
            if inb(PS2_STATUS) & 0x02 == 0 {
                return;
            }
        }
        core::hint::spin_loop();
    }
}

fn wait_read() {
    for _ in 0..10000 {
        unsafe {
            if inb(PS2_STATUS) & 0x01 != 0 {
                return;
            }
        }
        core::hint::spin_loop();
    }
}

fn write_command(cmd: u8) {
    wait_write();
    unsafe { outb(PS2_COMMAND, cmd); }
}

fn write_data(data: u8) {
    wait_write();
    unsafe { outb(PS2_DATA, data); }
}

fn read_data() -> u8 {
    wait_read();
    unsafe { inb(PS2_DATA) }
}

fn try_read_data() -> Option<u8> {
    for _ in 0..1000 {
        unsafe {
            if inb(PS2_STATUS) & 0x01 != 0 {
                return Some(inb(PS2_DATA));
            }
        }
        core::hint::spin_loop();
    }
    None
}

/// Initialize PS/2 mouse - minimal approach, don't touch keyboard
pub fn init() {
    use crate::info;

    // Just enable mouse port and IRQ12, nothing else
    write_command(0xA8); // Enable mouse port
    
    // Send commands directly to mouse
    write_command(0xD4);
    write_data(0xF4); // Enable data reporting
    let _ = try_read_data(); // ACK
    
    // Try scroll wheel magic
    for rate in [200u8, 100, 80] {
        write_command(0xD4);
        write_data(0xF3);
        let _ = try_read_data();
        write_command(0xD4);
        write_data(rate);
        let _ = try_read_data();
    }
    
    // Get device ID
    write_command(0xD4);
    write_data(0xF2);
    let _ = try_read_data();
    let device_id = try_read_data().unwrap_or(0);

    MOUSE_STATE.lock().has_scroll = device_id == 0x03 || device_id == 0x04;
    INITIALIZED.store(true, Ordering::Release);
    
    info!("Mouse: ID=0x{:02x}", device_id);
}

/// IRQ12 handler - called from interrupt handler
pub fn handle_irq() {
    if !INITIALIZED.load(Ordering::Acquire) {
        return;
    }

    let byte = unsafe { inb(PS2_DATA) };
    let mut state = MOUSE_STATE.lock();

    let cycle = state.cycle;
    state.bytes[cycle as usize] = byte;
    state.cycle += 1;

    let packet_size = if state.has_scroll { 4 } else { 3 };

    if state.cycle >= packet_size {
        state.cycle = 0;

        // Parse packet
        let flags = state.bytes[0];
        let dx = state.bytes[1] as i16;
        let dy = state.bytes[2] as i16;

        // Check for valid packet (bit 3 must be set)
        if flags & 0x08 == 0 {
            return;
        }

        // Handle scroll wheel
        if state.has_scroll && state.bytes[3] != 0 {
            let scroll = state.bytes[3] as i8;
            crate::serial::write_fmt(format_args!("Mouse scroll: {}\n", scroll));
            let _ = EVENT_QUEUE.lock().push(MouseEvent::Scroll { delta: scroll });
        }

        // Handle movement (only if significant)
        if dx.abs() > 2 || dy.abs() > 2 {
            let _ = EVENT_QUEUE.lock().push(MouseEvent::Move { dx, dy });
        }

        // Handle clicks
        if flags & 0x01 != 0 {
            let _ = EVENT_QUEUE.lock().push(MouseEvent::LeftClick);
        }
        if flags & 0x02 != 0 {
            let _ = EVENT_QUEUE.lock().push(MouseEvent::RightClick);
        }
        if flags & 0x04 != 0 {
            let _ = EVENT_QUEUE.lock().push(MouseEvent::MiddleClick);
        }
    }
}

/// Pop a mouse event from the queue
pub fn pop_event() -> Option<MouseEvent> {
    EVENT_QUEUE.lock().pop()
}

/// Check if mouse is initialized
pub fn is_initialized() -> bool {
    INITIALIZED.load(Ordering::Acquire)
}
