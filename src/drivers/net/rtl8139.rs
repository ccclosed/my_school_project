use core::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use spin::Mutex;
use x86::io::{inb, inw, inl, outb, outw, outl};
use crate::{debug, error, info, warn};

// RTL8139 I/O register offsets from I/O base
const CR: u16 = 0x37;       // Command Register (8-bit)
const CAPR: u16 = 0x38;     // Current Address of Packet Read (16-bit)
const CBR: u16 = 0x3A;      // Current Buffer Address (16-bit, read-only)
const IMR: u16 = 0x3C;      // Interrupt Mask Register (16-bit)
const ISR: u16 = 0x3E;      // Interrupt Status Register (16-bit)
const TCR: u16 = 0x40;      // Transmit Config Register (32-bit)
const RCR: u16 = 0x44;      // Receive Config Register (32-bit)
const TSD0: u16 = 0x10;     // TX Status Descriptor 0 (32-bit)
const TSD1: u16 = 0x14;     // TX Status Descriptor 1 (32-bit)
const TSD2: u16 = 0x18;     // TX Status Descriptor 2 (32-bit)
const TSD3: u16 = 0x1C;     // TX Status Descriptor 3 (32-bit)
const TSAD0: u16 = 0x20;    // TX Start Address 0 (32-bit)
const TSAD1: u16 = 0x24;    // TX Start Address 1 (32-bit)
const TSAD2: u16 = 0x28;    // TX Start Address 2 (32-bit)
const TSAD3: u16 = 0x2C;    // TX Start Address 3 (32-bit)
const RBSTART: u16 = 0x30;  // RX Buffer Start Address (32-bit)
const TPOLL: u16 = 0xD9;    // TX Poll (8-bit): write 0x40 to trigger TX

// Command Register bits
const CR_RST: u8 = 0x10;    // Reset
const CR_RE: u8 = 0x08;     // Receiver Enable
const CR_TE: u8 = 0x04;     // Transmitter Enable

const RX_BUF_SIZE: usize = 8192 + 16;

static INITIALIZED: AtomicBool = AtomicBool::new(false);
static IO_BASE: AtomicU16 = AtomicU16::new(0);

#[repr(align(16))]
struct RxBuf([u8; RX_BUF_SIZE]);
static RX_BUF: Mutex<RxBuf> = Mutex::new(RxBuf([0; RX_BUF_SIZE]));

/// Per-descriptor TX buffers — up to 4 concurrent TX ops without global lock contention.
#[repr(align(16))]
struct TxBuf([u8; 1514]);
static TX_BUFS: [Mutex<TxBuf>; 4] = [
    Mutex::new(TxBuf([0; 1514])),
    Mutex::new(TxBuf([0; 1514])),
    Mutex::new(TxBuf([0; 1514])),
    Mutex::new(TxBuf([0; 1514])),
];

// Alternate TX descriptors to work around RTL8139 repeated TX issue
static TX_DESC: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

/// Get the I/O base address (for use by net::rx_ring_pos).
/// SAFETY: only call after init().
pub unsafe fn io_base() -> u16 {
    IO_BASE.load(Ordering::Relaxed)
}

/// Return the physical address of a buffer reference.
/// The kernel uses identity mapping (virtual == physical), so the pointer IS the physical address.
fn phys_addr_of<T>(buf: &T) -> u32 {
    (buf as *const T) as u32
}

/// Reset the chip, read MAC, set up RX/TX. Returns the MAC address.
pub fn init(io_base: u16) -> [u8; 6] {
    IO_BASE.store(io_base, Ordering::Release);
    info!("RTL8139 init at I/O base 0x{:04x}", io_base);

    unsafe {
        // Software reset
        outb(io_base + CR, CR_RST);
        while inb(io_base + CR) & CR_RST != 0 {
            core::hint::spin_loop();
        }

        // Read MAC address from on-chip registers
        let mac_low = inl(io_base);
        let mac_high = inw(io_base + 0x04);
        let mac: [u8; 6] = [
            mac_low as u8,
            (mac_low >> 8) as u8,
            (mac_low >> 16) as u8,
            (mac_low >> 24) as u8,
            mac_high as u8,
            (mac_high >> 8) as u8,
        ];
        debug!(
            "RTL8139 MAC: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
        );

        // Point RBSTART to the RX ring buffer (physical address)
        let rx_addr = phys_addr_of(&*RX_BUF.lock());
        outl(io_base + RBSTART, rx_addr);

        // Unmask RX OK (bit 0) and TX OK (bit 2) interrupts
        outw(io_base + IMR, 0x0005);

        // RCR: Accept Physical Match + Broadcast + Multicast + All (promiscuous)
        //       buffer size = 8K+16, no wrap (bit 7 = 0)
        outl(io_base + RCR, 0x00000F0F);

        // TCR: interframe gap = 96 bits (default), CRC append, no loopback
        outl(io_base + TCR, 0x000000E0);

        // Enable RX and TX
        outb(io_base + CR, CR_TE | CR_RE);

        // Clear any pending interrupts
        outw(io_base + ISR, 0xFFFF);

        INITIALIZED.store(true, Ordering::Release);

        // Acknowledge the NIC by setting CAPR = CBR
        let cbr = inw(io_base + CBR);
        outw(io_base + CAPR, cbr);

        mac
    }
}

/// Enable or disable RTL8139 internal MAC loopback (TCR bits 9-8 = 01).
pub fn set_loopback(on: bool) {
    let io = IO_BASE.load(Ordering::Relaxed);
    unsafe {
        let mut tcr = inl(io + TCR);
        if on {
            tcr = (tcr & !0x300) | 0x100; // bits 9-8 = 01 = MAC loopback
        } else {
            tcr &= !0x300;                  // bits 9-8 = 00 = normal
            tcr |= 0x000000E0;              // restore default config
        }
        outl(io + TCR, tcr);
    }
}

/// Send an Ethernet frame. Blocks until TX completes.
/// Uses per-descriptor buffers — up to 4 callers can TX concurrently without contention.
pub fn send(frame: &[u8]) -> Result<(), ()> {
    if !INITIALIZED.load(Ordering::Acquire) {
        return Err(());
    }
    if frame.len() > 1514 {
        return Err(());
    }

    let io = IO_BASE.load(Ordering::Relaxed);
    let desc = TX_DESC.fetch_add(1, Ordering::Relaxed) as usize % 4;
    let (tsd, tsad) = match desc {
        0 => (TSD0, TSAD0),
        1 => (TSD1, TSAD1),
        2 => (TSD2, TSAD2),
        _ => (TSD3, TSAD3),
    };

    // Lock only this descriptor's buffer — other descriptors remain available
    let mut tx_buf = TX_BUFS[desc].lock();
    unsafe {
        tx_buf.0[..frame.len()].copy_from_slice(frame);
        let buf_addr = tx_buf.0.as_ptr() as u32;

        // Clear any stale interrupt flags
        outw(io + ISR, 0xFFFF);

        // Write TSAD + TSD with packet length and trigger TX
        outl(io + tsad, buf_addr);
        outl(io + tsd, frame.len() as u32);
        outb(io + TPOLL, 0x40);

        // Poll for TOK (bit 15) or TX Error Summary (bit 22)
        // using timer-based timeout instead of an iteration counter.
        let deadline = crate::timer::millis() + 100; // 100 ms timeout
        loop {
            let status = inl(io + tsd);
            if status & 0x8000 != 0 {
                debug!("RTL8139 TX OK (len={}, desc={})", frame.len(), desc);
                break;
            }
            if status & 0x0040_0000 != 0 {
                error!("RTL8139 TX error: status=0x{:08x}", status);
                return Err(());
            }
            if crate::timer::millis() >= deadline {
                warn!("RTL8139 TX timeout, status=0x{:08x} (desc={})", status, desc);
                return Err(());
            }
            core::hint::spin_loop();
        }
    }

    Ok(())
}

/// Volatile-read a u16 from an RX buffer pointer (NIC writes asynchronously).
unsafe fn rx_volatile_u16(ptr: *const u8, buf_start: *const u8, buf_end: *const u8) -> u16 {
    if ptr.is_null() || ptr < buf_start || ptr.add(1) >= buf_end {
        return 0;
    }
    let lo = core::ptr::read_volatile(ptr);
    let hi = core::ptr::read_volatile(ptr.add(1));
    u16::from_le_bytes([lo, hi])
}

/// Poll for a received packet. Returns the raw Ethernet frame data, or None.
pub fn poll_rx() -> Option<alloc::vec::Vec<u8>> {
    if !INITIALIZED.load(Ordering::Acquire) {
        return None;
    }
    let io = IO_BASE.load(Ordering::Relaxed);
    let rx_buf = RX_BUF.lock();
    let rx_ptr = rx_buf.0.as_ptr();

    unsafe {
        let capr = inw(io + CAPR) as usize;
        let cbr = inw(io + CBR) as usize;

        if capr == cbr {
            return None;
        }

        // I/O fence: NIC writes data to RX ring before updating CBR.
        // Guarantees volatile reads below see consistent data.
        core::sync::atomic::fence(Ordering::SeqCst);

        // Volatile-read header from the RX ring (written by NIC async)
        let buf_end = rx_ptr.add(RX_BUF_SIZE);
        let status = rx_volatile_u16(rx_ptr.add(capr), rx_ptr, buf_end);
        let pkt_size = rx_volatile_u16(rx_ptr.add(capr + 2), rx_ptr, buf_end) as usize;

        if pkt_size < 4 || pkt_size > 1518 + 4 {
            warn!("RTL8139 bad pkt: status=0x{:04x} size={}", status, pkt_size);
            outw(io + CAPR, cbr as u16);
            return None;
        }

        debug!("RTL8139 RX: status=0x{:04x} size={}", status, pkt_size);

        let actual_size = pkt_size - 4;
        let data_start = capr + 4;
        let mut data = alloc::vec::Vec::with_capacity(actual_size);

        // Copy data using volatile reads to prevent compiler optimization
        // since NIC can write to buffer asynchronously
        if data_start + actual_size <= RX_BUF_SIZE {
            for i in 0..actual_size {
                let byte = core::ptr::read_volatile(rx_ptr.add(data_start + i));
                data.push(byte);
            }
        } else {
            let first = RX_BUF_SIZE - data_start;
            for i in 0..first {
                let byte = core::ptr::read_volatile(rx_ptr.add(data_start + i));
                data.push(byte);
            }
            for i in 0..(actual_size - first) {
                let byte = core::ptr::read_volatile(rx_ptr.add(i));
                data.push(byte);
            }
        }

        // Advance CAPR past this packet, aligned to 4 bytes
        let next = (capr + 4 + pkt_size + 3) & !3;
        let new_capr = if next >= RX_BUF_SIZE { 0 } else { next };
        outw(io + CAPR, new_capr as u16);

        Some(data)
    }
}

/// Dump RTL8139 register state for debugging. Output goes to VGA + kernel.log.
pub fn dump_regs() {
    if !INITIALIZED.load(Ordering::Acquire) {
        info!("RTL8139: not initialized");
        return;
    }
    let io = IO_BASE.load(Ordering::Relaxed);
    unsafe {
        let cr = inb(io + CR);
        let tcr = inl(io + TCR);
        let rcr = inl(io + RCR);
        let isr = inw(io + ISR);
        let imr = inw(io + IMR);
        let tsd0 = inl(io + TSD0);
        let capr = inw(io + CAPR);
        let cbr = inw(io + CBR);
        let rbst = inl(io + RBSTART);
        let mac0 = inl(io);
        let mac4 = inw(io + 0x04);
        info!("RTL8139 registers:");
        info!("  CR     = 0x{:02x} (TE={} RE={} RST={})",
            cr, (cr >> 2) & 1, (cr >> 3) & 1, (cr >> 4) & 1);
        info!("  TCR    = 0x{:08x}", tcr);
        info!("  RCR    = 0x{:08x}", rcr);
        info!("  ISR    = 0x{:04x}", isr);
        info!("  IMR    = 0x{:04x}", imr);
        info!("  TSD0   = 0x{:08x} (TOK={} OWN={} TER={})",
            tsd0, (tsd0 >> 15) & 1, (tsd0 >> 14) & 1, (tsd0 >> 22) & 1);
        info!("  CAPR   = {}", capr);
        info!("  CBR    = {}", cbr);
        info!("  RBSTART= 0x{:08x}", rbst);
        info!("  MAC    = {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            mac0 as u8, (mac0 >> 8) as u8, (mac0 >> 16) as u8,
            (mac0 >> 24) as u8, mac4 as u8, (mac4 >> 8) as u8);
    }
}

#[allow(dead_code)]
pub fn default_mac() -> [u8; 6] {
    [0x52, 0x54, 0x00, 0xAB, 0xCD, 0xEF]
}
