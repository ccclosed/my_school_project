use alloc::format;
use alloc::string::{String, ToString};

use crate::fs::ramfs;
use crate::keyboard::{self, KeyEvent};
use crate::vga;

const EDITOR_ROWS: usize = 24;

pub fn run(path: &str) {
    let mut buffer = match ramfs::read(path) {
        Ok(data) => String::from_utf8_lossy(&data).into_owned(),
        Err(_) => String::new(),
    };
    if !buffer.ends_with('\n') && !buffer.is_empty() {
        buffer.push('\n');
    }

    let mut cursor = buffer.len();
    let mut row: usize = 0;
    let mut col: usize = 0;
    let mut dirty = false;

    keyboard::flush();
    redraw(&buffer, path);
    vga::set_cursor(0, 0);

    loop {
        match keyboard::pop_event() {
            Some(KeyEvent::Escape) => break,
            Some(KeyEvent::Ctrl('s')) | Some(KeyEvent::Ctrl('S')) => {
                let data = buffer.as_bytes();
                let _ = ramfs::write(path, data);
                dirty = false;
                vga::status_line(&format!("Saved {}", path));
            }
            Some(KeyEvent::Up) => {
                if row > 0 {
                    row -= 1;
                    col = col.min(line_len(&buffer, row));
                    vga::set_cursor(row, col);
                }
            }
            Some(KeyEvent::Down) => {
                if row + 1 < line_count(&buffer) {
                    row += 1;
                    col = col.min(line_len(&buffer, row));
                    vga::set_cursor(row, col);
                }
            }
            Some(KeyEvent::Left) => {
                if col > 0 {
                    col -= 1;
                    vga::set_cursor(row, col);
                } else if row > 0 {
                    row -= 1;
                    col = line_len(&buffer, row);
                    vga::set_cursor(row, col);
                }
            }
            Some(KeyEvent::Right) => {
                if col < line_len(&buffer, row) {
                    col += 1;
                    vga::set_cursor(row, col);
                } else if row + 1 < line_count(&buffer) {
                    row += 1;
                    col = 0;
                    vga::set_cursor(row, col);
                }
            }
            Some(KeyEvent::Backspace) => {
                if cursor > 0 {
                    buffer.remove(cursor - 1);
                    cursor -= 1;
                    dirty = true;
                    (row, col) = offset_to_pos(&buffer, cursor);
                    redraw(&buffer, path);
                    vga::set_cursor(row, col);
                }
            }
            Some(KeyEvent::Enter) => {
                buffer.insert(cursor, '\n');
                cursor += 1;
                dirty = true;
                (row, col) = offset_to_pos(&buffer, cursor);
                redraw(&buffer, path);
                vga::set_cursor(row, col);
            }
            Some(KeyEvent::Char(c)) => {
                buffer.insert(cursor, c as char);
                cursor += 1;
                dirty = true;
                (row, col) = offset_to_pos(&buffer, cursor);
                redraw(&buffer, path);
                vga::set_cursor(row, col);
            }
            None => crate::arch::hlt(),
            _ => {}
        }
    }

    vga::clear_screen();
    if dirty {
        println!("Editor closed (unsaved changes discarded unless Ctrl+S was used)");
    }
}

fn redraw(buffer: &str, path: &str) {
    for r in 0..EDITOR_ROWS {
        vga::clear_line(r);
        if let Some(line) = buffer.lines().nth(r) {
            vga::write_at(r, 0, line);
        }
    }
    let status = if path.is_empty() {
        "editor".to_string()
    } else {
        alloc::format!("edit: {} | Esc=exit Ctrl+S=save", path)
    };
    vga::status_line(&status);
}

fn line_count(s: &str) -> usize {
    s.lines().count().max(1)
}

fn line_len(s: &str, row: usize) -> usize {
    s.lines().nth(row).map(|l| l.len()).unwrap_or(0)
}

fn offset_to_pos(s: &str, offset: usize) -> (usize, usize) {
    let mut i = 0;
    for (r, line) in s.lines().enumerate() {
        let line_bytes = line.len() + 1;
        if i + line_bytes > offset {
            return (r, offset - i);
        }
        i += line_bytes;
    }
    let rows = line_count(s);
    (rows.saturating_sub(1), line_len(s, rows.saturating_sub(1)))
}
