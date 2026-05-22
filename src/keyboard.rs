use core::sync::atomic::{AtomicU32, AtomicUsize, AtomicBool, Ordering};
use x86::io::inb;

use crate::arch;

const PS2_DATA: u16 = 0x60;
const BUF_SIZE: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyEvent {
    Char(u8),
    Up,
    Down,
    Left,
    Right,
    Backspace,
    Enter,
    Escape,
    Ctrl(char),
    PageUp,
    PageDown,
}

fn encode(ev: KeyEvent) -> u32 {
    match ev {
        KeyEvent::Char(c) => 1 << 8 | c as u32,
        KeyEvent::Up => 2 << 8,
        KeyEvent::Down => 3 << 8,
        KeyEvent::Left => 4 << 8,
        KeyEvent::Right => 5 << 8,
        KeyEvent::Backspace => 6 << 8,
        KeyEvent::Enter => 7 << 8,
        KeyEvent::Escape => 8 << 8,
        KeyEvent::Ctrl(c) => 9 << 8 | c as u32,
        KeyEvent::PageUp => 10 << 8,
        KeyEvent::PageDown => 11 << 8,
    }
}

fn decode(val: u32) -> Option<KeyEvent> {
    if val == 0 {
        return None;
    }
    let typ = val >> 8;
    let data = val as u8;
    match typ {
        1 => Some(KeyEvent::Char(data)),
        2 => Some(KeyEvent::Up),
        3 => Some(KeyEvent::Down),
        4 => Some(KeyEvent::Left),
        5 => Some(KeyEvent::Right),
        6 => Some(KeyEvent::Backspace),
        7 => Some(KeyEvent::Enter),
        8 => Some(KeyEvent::Escape),
        9 => Some(KeyEvent::Ctrl(data as char)),
        10 => Some(KeyEvent::PageUp),
        11 => Some(KeyEvent::PageDown),
        _ => None,
    }
}

/// Lock-free SPSC ring buffer: producer (IRQ1) pushes, consumer (shell) pops.
struct LockFreeRing {
    buf: [AtomicU32; BUF_SIZE],
    head: AtomicUsize,
    tail: AtomicUsize,
}

unsafe impl Sync for LockFreeRing {}

impl LockFreeRing {
    fn push(&self, ev: KeyEvent) {
        let h = self.head.load(Ordering::Relaxed);
        let t = self.tail.load(Ordering::Acquire);
        let next = (h + 1) % BUF_SIZE;
        if next == t {
            return;
        }
        self.buf[h].store(encode(ev), Ordering::Release);
        self.head.store(next, Ordering::Release);
    }

    fn pop(&self) -> Option<KeyEvent> {
        let t = self.tail.load(Ordering::Relaxed);
        let h = self.head.load(Ordering::Acquire);
        if t == h {
            return None;
        }
        let val = self.buf[t].load(Ordering::Acquire);
        self.tail.store((t + 1) % BUF_SIZE, Ordering::Release);
        decode(val)
    }

    fn clear(&self) {
        let h = self.head.load(Ordering::Acquire);
        self.tail.store(h, Ordering::Release);
    }
}

static KEYBUF: LockFreeRing = LockFreeRing {
    buf: unsafe { core::mem::transmute::<[u32; BUF_SIZE], [AtomicU32; BUF_SIZE]>([0; BUF_SIZE]) },
    head: AtomicUsize::new(0),
    tail: AtomicUsize::new(0),
};

static SHIFT: AtomicBool = AtomicBool::new(false);
static CTRL: AtomicBool = AtomicBool::new(false);
/// Set when a 0xE0 extended-prefix byte was received.
static EXTENDED: AtomicBool = AtomicBool::new(false);

pub fn handle_irq() {
    // Only read if data is from keyboard (status bit 5 = 0 means keyboard)
    let status = unsafe { x86::io::inb(0x64) };
    if status & 0x20 != 0 {
        return; // mouse data, let mouse handler deal with it
    }
    let sc = unsafe { inb(PS2_DATA) };

    if sc == 0xE0 {
        EXTENDED.store(true, Ordering::Relaxed);
        return;
    }

    if EXTENDED.swap(false, Ordering::Relaxed) {
        if let Some(ev) = translate_extended(sc) {
            KEYBUF.push(ev);
        }
        return;
    }
    
    if sc & 0x80 != 0 {
        handle_release(sc & 0x7F);
        return;
    }
    
    if let Some(ev) = translate(sc) {
        KEYBUF.push(ev);
    }
}

fn handle_release(sc: u8) {
    match sc {
        0x2A | 0x36 => SHIFT.store(false, Ordering::Relaxed),
        0x1D => CTRL.store(false, Ordering::Relaxed),
        _ => {}
    }
}

fn translate_extended(sc: u8) -> Option<KeyEvent> {
    match sc {
        0x48 => Some(KeyEvent::Up),
        0x50 => Some(KeyEvent::Down),
        0x4B => Some(KeyEvent::Left),
        0x4D => Some(KeyEvent::Right),
        0x49 => Some(KeyEvent::PageUp),
        0x51 => Some(KeyEvent::PageDown),
        _ => None,
    }
}

pub fn pop_event() -> Option<KeyEvent> {
    KEYBUF.pop()
}

#[allow(dead_code)]
pub fn read_line(buf: &mut alloc::string::String) {
    loop {
        if let Some(ev) = pop_event() {
            match ev {
                KeyEvent::Char(c) => {
                    print!("{}", c as char);
                    buf.push(c as char);
                }
                KeyEvent::Backspace => {
                    if !buf.is_empty() {
                        buf.pop();
                        crate::vga::backspace();
                    }
                }
                KeyEvent::Enter => {
                    println!();
                    return;
                }
                _ => {}
            }
        }
        arch::hlt();
    }
}

fn translate(sc: u8) -> Option<KeyEvent> {
    match sc {
        0x2A | 0x36 => {
            SHIFT.store(true, Ordering::Relaxed);
            None
        }
        0x1D => {
            CTRL.store(true, Ordering::Relaxed);
            None
        }
        0x0E => Some(KeyEvent::Backspace),
        0x1C => Some(KeyEvent::Enter),
        0x01 => Some(KeyEvent::Escape),
        // Arrow keys are handled by translate_extended (0xE0 prefix)
        // Remove duplicate handling here
        _ => {
            let shift = SHIFT.load(Ordering::Relaxed);
            let ctrl = CTRL.load(Ordering::Relaxed);
            let c = scancode_to_ascii(sc, shift)?;
            if ctrl {
                Some(KeyEvent::Ctrl(c as char))
            } else {
                Some(KeyEvent::Char(c))
            }
        }
    }
}

fn scancode_to_ascii(sc: u8, shift: bool) -> Option<u8> {
    const LOWER: [u8; 58] = [
        0, 27, b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', b'0', b'-', b'=', 8,
        b'\t', b'q', b'w', b'e', b'r', b't', b'y', b'u', b'i', b'o', b'p', b'[', b']', b'\n',
        0, b'a', b's', b'd', b'f', b'g', b'h', b'j', b'k', b'l', b';', b'\'', b'`', 0,
        b'\\', b'z', b'x', b'c', b'v', b'b', b'n', b'm', b',', b'.', b'/', 0, b'*', 0, b' ',
    ];
    const UPPER: [u8; 58] = [
        0, 27, b'!', b'@', b'#', b'$', b'%', b'^', b'&', b'*', b'(', b')', b'_', b'+', 8,
        b'\t', b'Q', b'W', b'E', b'R', b'T', b'Y', b'U', b'I', b'O', b'P', b'{', b'}', b'\n',
        0, b'A', b'S', b'D', b'F', b'G', b'H', b'J', b'K', b'L', b':', b'"', b'~', 0,
        b'|', b'Z', b'X', b'C', b'V', b'B', b'N', b'M', b'<', b'>', b'?', 0, b'*', 0, b' ',
    ];
    if sc as usize >= LOWER.len() {
        return None;
    }
    let c = if shift { UPPER[sc as usize] } else { LOWER[sc as usize] };
    if c == 0 || c == 27 {
        None
    } else {
        Some(c)
    }
}

pub fn flush() {
    KEYBUF.clear();
}
