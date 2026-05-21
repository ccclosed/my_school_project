use core::fmt;
use lazy_static::lazy_static;
use spin::Mutex;
use x86::io::outb;
extern crate alloc;

const BUFFER: *mut u8 = 0xB8000 as *mut u8;
const WIDTH: usize = 80;
const HEIGHT: usize = 25;
const CONTENT_ROWS: usize = HEIGHT - 1; // line 24 reserved for status bar

// Scrollback buffer - stores last 500 lines
const SCROLLBACK_LINES: usize = 500;
static SCROLLBACK: Mutex<ScrollbackBuffer> = Mutex::new(ScrollbackBuffer::new());

struct ScrollbackBuffer {
    lines: [[u8; WIDTH * 2]; SCROLLBACK_LINES], // char + color for each cell
    current_line: usize,
    scroll_offset: usize, // How many lines scrolled back
}

impl ScrollbackBuffer {
    const fn new() -> Self {
        Self {
            lines: [[0; WIDTH * 2]; SCROLLBACK_LINES],
            current_line: 0,
            scroll_offset: 0,
        }
    }

    fn add_line(&mut self, line_data: &[u8; WIDTH * 2]) {
        self.lines[self.current_line % SCROLLBACK_LINES] = *line_data;
        self.current_line += 1;
        // Reset scroll when new content arrives
        if self.scroll_offset > 0 {
            self.scroll_offset = 0;
        }
    }

    fn scroll_up(&mut self, lines: usize) {
        let max_scroll = self.current_line.saturating_sub(CONTENT_ROWS);
        self.scroll_offset = (self.scroll_offset + lines).min(max_scroll);
    }

    fn scroll_down(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    fn get_line(&self, visible_row: usize) -> Option<&[u8; WIDTH * 2]> {
        if self.current_line < CONTENT_ROWS {
            // Not enough history yet
            return None;
        }
        let history_line = self.current_line.saturating_sub(CONTENT_ROWS)
            .saturating_sub(self.scroll_offset)
            + visible_row;
        
        if history_line >= self.current_line {
            return None;
        }
        
        Some(&self.lines[history_line % SCROLLBACK_LINES])
    }
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Color {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}

#[derive(Clone, Copy)]
struct ColorCode(u8);

impl ColorCode {
    fn new(fg: Color, bg: Color) -> Self {
        ColorCode((bg as u8) << 4 | (fg as u8))
    }
}

pub(crate) struct Writer {
    row: usize,
    col: usize,
    color: ColorCode,
}

impl Writer {
    fn new() -> Self {
        Writer {
            row: 0,
            col: 0,
            color: ColorCode::new(Color::LightGray, Color::Black),
        }
    }

    fn clear(&mut self) {
        for row in 0..CONTENT_ROWS {
            for col in 0..WIDTH {
                self.write_at(row, col, b' ', self.color);
            }
        }
        self.row = 0;
        self.col = 0;
    }

    fn scroll(&mut self) {
        // Save the top line to deferred scrollback — SCROLLBACK.lock()
        // must not be acquired inside WRITER.lock() to avoid deadlocks.
        let mut line_data = [0u8; WIDTH * 2];
        unsafe {
            for i in 0..WIDTH * 2 {
                line_data[i] = core::ptr::read_volatile(BUFFER.add(i));
            }
        }
        *DEFERRED_SCROLL_LINE.lock() = Some(line_data);

        unsafe {
            core::ptr::copy_nonoverlapping(
                BUFFER.add(WIDTH * 2),
                BUFFER,
                (CONTENT_ROWS - 1) * WIDTH * 2,
            );
        }
        let blank = ColorCode::new(Color::LightGray, Color::Black);
        for col in 0..WIDTH {
            self.write_at(CONTENT_ROWS - 1, col, b' ', blank);
        }
    }

    fn write_at(&self, row: usize, col: usize, byte: u8, color: ColorCode) {
        let offset = (row * WIDTH + col) * 2;
        unsafe {
            core::ptr::write_volatile(BUFFER.add(offset), byte);
            core::ptr::write_volatile(BUFFER.add(offset + 1), color.0);
        }
    }

    fn newline(&mut self) {
        self.col = 0;
        if self.row + 1 >= CONTENT_ROWS {
            self.scroll();
        } else {
            self.row += 1;
        }
        update_hardware_cursor(self.row, self.col);
    }

    fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.newline(),
            b'\r' => self.col = 0,
            b'\t' => {
                self.col = (self.col + 8) & !7;
                if self.col >= WIDTH {
                    self.newline();
                }
            }
            byte => {
                let byte = if byte.is_ascii() && byte != 0x7F {
                    byte
                } else {
                    b'?'
                };
                if self.col >= WIDTH {
                    self.newline();
                }
                self.write_at(self.row, self.col, byte, self.color);
                self.col += 1;
            }
        }
    }

    fn write_str(&mut self, s: &str) {
        for &b in s.as_bytes() {
            self.write_byte(b);
        }
        update_hardware_cursor(self.row, self.col);
    }

    pub fn set_color(&mut self, fg: Color, bg: Color) {
        self.color = ColorCode::new(fg, bg);
    }
}

// Deferred scrollback line: scroll() saves here while WRITER is locked;
// commit after releasing WRITER to avoid SCROLLBACK.lock() inside WRITER.lock().
static DEFERRED_SCROLL_LINE: Mutex<Option<[u8; WIDTH * 2]>> = Mutex::new(None);

fn commit_scrollback() {
    if let Some(line) = DEFERRED_SCROLL_LINE.lock().take() {
        SCROLLBACK.lock().add_line(&line);
    }
}

lazy_static! {
    static ref WRITER: Mutex<Writer> = Mutex::new(Writer::new());
}

fn write_crtc(index: u8, value: u8) {
    unsafe {
        outb(0x3D4, index);
        outb(0x3D5, value);
    }
}

/// Reset BIOS CRTC start address and hardware cursor (fixes first column blank).
fn init_hardware() {
    write_crtc(0x0C, 0);
    write_crtc(0x0D, 0);
    update_hardware_cursor(0, 0);
}

fn update_hardware_cursor(row: usize, col: usize) {
    let pos = (row * WIDTH + col) as u16;
    write_crtc(0x0E, (pos >> 8) as u8);
    write_crtc(0x0F, pos as u8);
}

pub fn init() {
    init_hardware();
    WRITER.lock().clear();
    update_hardware_cursor(0, 0);
}

pub fn write_fmt(args: core::fmt::Arguments) {
    use core::fmt::Write;
    let _ = WRITER.lock().write_fmt(args);
    commit_scrollback();
}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_str(s);
        Ok(())
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        $crate::vga::write_fmt(format_args!($($arg)*));
        $crate::serial::write_fmt(format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => {{
        $crate::print!($($arg)*);
        $crate::print!("\n");
    }};
}

pub fn clear_screen() {
    WRITER.lock().clear();
    commit_scrollback();
}

pub fn set_color(fg: Color, bg: Color) {
    WRITER.lock().set_color(fg, bg);
    commit_scrollback();
}

pub fn set_cursor(row: usize, col: usize) {
    let mut w = WRITER.lock();
    w.row = row.min(CONTENT_ROWS - 1);
    w.col = col.min(WIDTH - 1);
    update_hardware_cursor(w.row, w.col);
    drop(w);
    commit_scrollback();
}

#[allow(dead_code)]
pub fn cursor_position() -> (usize, usize) {
    let w = WRITER.lock();
    (w.row, w.col)
}

pub fn write_at(row: usize, col: usize, s: &str) {
    let row = row.min(CONTENT_ROWS - 1);
    let w = WRITER.lock();
    for (i, &b) in s.as_bytes().iter().enumerate() {
        if col + i < WIDTH {
            w.write_at(row, col + i, b, w.color);
        }
    }
    drop(w);
    commit_scrollback();
}

pub fn clear_line(row: usize) {
    let row = row.min(CONTENT_ROWS - 1);
    let w = WRITER.lock();
    for col in 0..WIDTH {
        w.write_at(row, col, b' ', w.color);
    }
    drop(w);
    commit_scrollback();
}

pub fn status_line(text: &str) {
    let mut w = WRITER.lock();
    let color = w.color;
    w.set_color(Color::Black, Color::LightGray);
    for col in 0..WIDTH {
        w.write_at(HEIGHT - 1, col, b' ', ColorCode::new(Color::Black, Color::LightGray));
    }
    for (i, &b) in text.as_bytes().iter().enumerate() {
        if i < WIDTH {
            w.write_at(
                HEIGHT - 1,
                i,
                b,
                ColorCode::new(Color::Black, Color::LightGray),
            );
        }
    }
    w.color = color;
    drop(w);
    commit_scrollback();
}

pub fn backspace() {
    let mut w = WRITER.lock();
    if w.col > 0 {
        w.col -= 1;
        w.write_at(w.row, w.col, b' ', w.color);
    } else if w.row > 0 {
        w.row -= 1;
        w.col = WIDTH - 1;
        w.write_at(w.row, w.col, b' ', w.color);
    }
    update_hardware_cursor(w.row, w.col);
    drop(w);
    commit_scrollback();
}

/// Scroll the display up (show older content)
pub fn scroll_display_up(lines: usize) {
    let mut sb = SCROLLBACK.lock();
    sb.scroll_up(lines);
    let offset = sb.scroll_offset;
    redraw_from_scrollback(&sb);
    drop(sb);

    if offset > 0 {
        status_line(&alloc::format!("  SCROLLBACK: -{} lines (scroll down to return)", offset));
    }
}

/// Scroll the display down (show newer content)
pub fn scroll_display_down(lines: usize) {
    let mut sb = SCROLLBACK.lock();
    sb.scroll_down(lines);
    let offset = sb.scroll_offset;
    if offset == 0 {
        drop(sb);
        status_line("");
    } else {
        redraw_from_scrollback(&sb);
        drop(sb);
        status_line(&alloc::format!("  SCROLLBACK: -{} lines (scroll down to return)", offset));
    }
}

/// Redraw screen from scrollback buffer
fn redraw_from_scrollback(sb: &ScrollbackBuffer) {
    if sb.scroll_offset == 0 {
        return; // Live view, no need to redraw
    }

    for row in 0..CONTENT_ROWS {
        if let Some(line_data) = sb.get_line(row) {
            for i in 0..WIDTH * 2 {
                unsafe { core::ptr::write_volatile(BUFFER.add(row * WIDTH * 2 + i), line_data[i]); }
            }
        }
    }

    // Reset WRITER's cursor position and update hardware cursor
    let mut w = WRITER.lock();
    w.row = 0;
    w.col = 0;
    write_crtc(0x0E, 0);
    write_crtc(0x0F, 0);
}
