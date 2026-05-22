//! ATA PIO driver — read sectors from an IDE/SATA disk.
//!
//! Uses the primary ATA bus (I/O ports 0x1F0–0x1F7) in PIO mode.
//! 28-bit LBA addressing (up to 128 GB).
//!
//! QEMU: `-drive file=disk.img,format=raw,if=ide`

use crate::{info, warn, debug};
use x86::io;

const DATA: u16       = 0x1F0;
const ERROR: u16      = 0x1F1;
const SECTOR_COUNT: u16 = 0x1F2;
const LBA_LO: u16     = 0x1F3;
const LBA_MID: u16    = 0x1F4;
const LBA_HI: u16     = 0x1F5;
const DRIVE: u16      = 0x1F6;
const STATUS: u16     = 0x1F7;

const STATUS_ERR: u8 = 0x01;
const STATUS_DRQ: u8 = 0x08;
const STATUS_DRDY: u8 = 0x40;
const STATUS_BSY: u8  = 0x80;

const CMD_READ_SECTORS: u8  = 0x20;
const CMD_WRITE_SECTORS: u8 = 0x30;
const DRIVE_MASTER: u8 = 0xE0;

unsafe fn wait_ready() -> Result<(), &'static str> {
    let deadline = crate::timer::millis() + 4000;
    loop {
        let status = io::inb(STATUS);
        if status & STATUS_ERR != 0 {
            let err = io::inb(ERROR);
            debug!("ATA error: status=0x{:02x} error=0x{:02x}", status, err);
            return Err("ATA error");
        }
        if status & STATUS_BSY == 0 && status & STATUS_DRDY != 0 {
            return Ok(());
        }
        if crate::timer::millis() > deadline {
            return Err("ATA timeout");
        }
        core::hint::spin_loop();
    }
}

unsafe fn wait_drq() -> Result<(), &'static str> {
    let deadline = crate::timer::millis() + 4000;
    loop {
        let status = io::inb(STATUS);
        if status & STATUS_ERR != 0 {
            return Err("ATA error");
        }
        if status & STATUS_DRQ != 0 {
            return Ok(());
        }
        if status & STATUS_BSY == 0 && status & STATUS_DRDY != 0 && status & STATUS_DRQ == 0 {
            return Err("ATA no data");
        }
        if crate::timer::millis() > deadline {
            return Err("ATA DRQ timeout");
        }
        core::hint::spin_loop();
    }
}

pub unsafe fn read_sectors(lba: u32, count: u8, buf: &mut [u8]) -> Result<(), &'static str> {
    if count == 0 || buf.len() < count as usize * 512 {
        return Err("ATA: buffer too small");
    }

    wait_ready()?;
    io::outb(DRIVE, DRIVE_MASTER | ((lba >> 24) & 0x0F) as u8);
    io::outb(SECTOR_COUNT, count);
    io::outb(LBA_LO, lba as u8);
    io::outb(LBA_MID, (lba >> 8) as u8);
    io::outb(LBA_HI, (lba >> 16) as u8);
    io::outb(STATUS, CMD_READ_SECTORS);

    for sector in 0..count as usize {
        wait_drq()?;
        let offset = sector * 512;
        let ptr = buf.as_mut_ptr().add(offset) as *mut u16;
        for w in 0..256 {
            core::ptr::write_volatile(ptr.add(w), io::inw(DATA));
        }
        io::inb(STATUS);
    }
    Ok(())
}

/// Write `count` sectors starting at LBA `lba` from `buf`.
pub unsafe fn write_sectors(lba: u32, count: u8, buf: &[u8]) -> Result<(), &'static str> {
    if count == 0 || buf.len() < count as usize * 512 {
        return Err("ATA: buffer too small");
    }

    wait_ready()?;
    io::outb(DRIVE, DRIVE_MASTER | ((lba >> 24) & 0x0F) as u8);
    io::outb(SECTOR_COUNT, count);
    io::outb(LBA_LO, lba as u8);
    io::outb(LBA_MID, (lba >> 8) as u8);
    io::outb(LBA_HI, (lba >> 16) as u8);
    io::outb(STATUS, CMD_WRITE_SECTORS);

    for sector in 0..count as usize {
        wait_drq()?;
        let offset = sector * 512;
        let ptr = buf.as_ptr().add(offset) as *const u16;
        for w in 0..256 {
            io::outw(DATA, core::ptr::read_volatile(ptr.add(w)));
        }
        io::inb(STATUS);
    }

    // Wait for write to complete (BSY clears)
    wait_ready()?;

    Ok(())
}

use spin::Mutex;
static INITIALIZED: Mutex<bool> = Mutex::new(false);

pub fn init() {
    let mut init = INITIALIZED.lock();
    if *init {
        return;
    }
    let status = unsafe { io::inb(STATUS) };
    if status == 0xFF {
        warn!("ATA: no drive (status=0xFF)");
        return;
    }
    *init = true;
    info!("ATA: drive ready (status=0x{:02x})", status);
}

pub fn write(lba: u32, count: u8, buf: &[u8]) -> Result<(), &'static str> {
    if !*INITIALIZED.lock() {
        init();
    }
    unsafe { write_sectors(lba, count, buf) }
}

pub fn read(lba: u32, count: u8, buf: &mut [u8]) -> Result<(), &'static str> {
    if !*INITIALIZED.lock() {
        init();
    }
    unsafe { read_sectors(lba, count, buf) }
}
