/// CMOS RTC driver — reads current time from the MC146818-compatible RTC.
/// Ports: 0x70 (address), 0x71 (data).
use x86::io::{inb, outb};

#[derive(Clone, Copy, Debug)]
pub struct RtcTime {
    pub hours: u8,
    pub minutes: u8,
    pub seconds: u8,
}

fn bcd_to_bin(bcd: u8) -> u8 {
    (bcd / 16) * 10 + (bcd % 16)
}

fn read_reg(reg: u8) -> u8 {
    unsafe {
        outb(0x70, reg);
        inb(0x71)
    }
}

/// Read the current time from CMOS RTC with consistency check.
/// Double-reads seconds to detect rollover during read.
pub fn read() -> RtcTime {
    // Register B tells us if data is in BCD or binary and if 12/24h mode
    let reg_b = read_reg(0x0B);
    let is_binary = (reg_b & 0x04) != 0;
    let is_24h = (reg_b & 0x02) != 0;

    // Wait for RTC to finish updating (UIP bit clear), with timeout
    for _ in 0..100_000 {
        if read_reg(0x0A) & 0x80 == 0 {
            break;
        }
        core::hint::spin_loop();
    }

    let sec1 = read_reg(0x00);
    let mut sec = sec1;
    let mut min = read_reg(0x02);
    let mut hour = read_reg(0x04);

    // Double-read seconds; if changed, RTC was updating — re-read everything
    let sec2 = read_reg(0x00);
    if sec2 != sec1 {
        for _ in 0..100_000 {
            if read_reg(0x0A) & 0x80 == 0 {
                break;
            }
            core::hint::spin_loop();
        }
        sec = read_reg(0x00);
        min = read_reg(0x02);
        hour = read_reg(0x04);
    }

    if !is_binary {
        sec = bcd_to_bin(sec);
        min = bcd_to_bin(min);
        hour = bcd_to_bin(hour);
    }

    // Convert 12h format to 24h if needed
    if !is_24h {
        let pm = hour & 0x80 != 0;
        hour &= 0x7F;
        if pm {
            hour += 12;
            if hour == 24 {
                hour = 12;
            }
        } else if hour == 12 {
            hour = 0;
        }
    }

    RtcTime { hours: hour, minutes: min, seconds: sec }
}
