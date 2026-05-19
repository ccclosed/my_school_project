mod calc;
mod editor;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::cpu;
use crate::drivers::net::{self, NicKind};
use crate::drivers::net::{arp, ethernet};
use crate::fs::ramfs;
use crate::keyboard;
use crate::keyboard::KeyEvent;
use crate::memory;
use crate::serial;
use crate::vga;

const PROMPT: &str = "kernel> ";
const MAX_HISTORY: usize = 16;

struct History {
    items: Vec<String>,
    index: isize,
}

impl History {
    const fn new() -> Self {
        History {
            items: Vec::new(),
            index: -1,
        }
    }

    fn push(&mut self, cmd: &str) {
        if cmd.is_empty() {
            self.index = -1;
            return;
        }
        if let Some(last) = self.items.last() {
            if last.as_str() == cmd {
                self.index = -1;
                return;
            }
        }
        self.items.push(cmd.to_string());
        if self.items.len() > MAX_HISTORY {
            self.items.drain(0..1);
        }
        self.index = -1;
    }

    fn prev(&mut self) -> Option<&str> {
        let len = self.items.len();
        if len == 0 {
            return None;
        }
        if self.index < 0 {
            self.index = (len - 1) as isize;
        } else if self.index > 0 {
            self.index -= 1;
        }
        Some(self.items[self.index as usize].as_str())
    }

    fn next(&mut self) -> Option<&str> {
        let len = self.items.len();
        if len == 0 || self.index < 0 {
            return None;
        }
        if self.index as usize >= len - 1 {
            self.index = -1;
            return None;
        }
        self.index += 1;
        Some(self.items[self.index as usize].as_str())
    }
}

fn load_line(buf: &mut [u8], len: &mut usize, s: &str) {
    for _ in 0..*len {
        vga::backspace();
    }
    *len = 0;
    for &b in s.as_bytes() {
        if *len < buf.len() {
            buf[*len] = b;
            *len += 1;
            print!("{}", b as char);
        }
    }
}

pub fn run() -> ! {
    let mut history = History::new();

    loop {
        print!("{}", PROMPT);
        let mut line_buf: [u8; 256] = [0; 256];
        let mut line_len: usize = 0;

        loop {
            match keyboard::pop_event() {
                Some(KeyEvent::Char(c)) => {
                    if line_len < line_buf.len() {
                        line_buf[line_len] = c;
                        line_len += 1;
                        print!("{}", c as char);
                    }
                }
                Some(KeyEvent::Backspace) => {
                    if line_len > 0 {
                        line_len -= 1;
                        vga::backspace();
                    }
                }
                Some(KeyEvent::Enter) => {
                    println!();
                    break;
                }
                Some(KeyEvent::Up) => {
                    if let Some(cmd) = history.prev() {
                        load_line(&mut line_buf, &mut line_len, cmd);
                    }
                }
                Some(KeyEvent::Down) => {
                    if let Some(cmd) = history.next() {
                        load_line(&mut line_buf, &mut line_len, cmd);
                    } else {
                        for _ in 0..line_len { vga::backspace(); }
                        line_len = 0;
                    }
                }
                Some(KeyEvent::Ctrl('c')) => {
                    for _ in 0..line_len { vga::backspace(); }
                    println!("^C");
                    line_len = 0;
                    break;
                }
                _ => {}
            }

            if let Some(byte) = serial::poll_char() {
                match byte {
                    b'\r' | b'\n' => { println!(); break; }
                    0x7F | 0x08 => {
                        if line_len > 0 { line_len -= 1; vga::backspace(); }
                    }
                    0x03 => {
                        for _ in 0..line_len { vga::backspace(); }
                        println!("^C");
                        line_len = 0;
                        break;
                    }
                    c if c >= 0x20 && c <= 0x7E => {
                        if line_len < line_buf.len() {
                            line_buf[line_len] = c;
                            line_len += 1;
                            print!("{}", c as char);
                        }
                    }
                    _ => {}
                }
            }

            crate::arch::hlt();
        }

        let trimmed = core::str::from_utf8(&line_buf[..line_len]).unwrap_or("").trim();
        if !trimmed.is_empty() {
            if trimmed.chars().all(|c| c != '\0' && (!c.is_control() || c == '\n' || c == '\t')) {
                // Avoid allocations - work with &str directly
                history.push(trimmed);
                dispatch(trimmed);
            } else {
                println!("Invalid input");
            }
        }
    }
}

fn dispatch(line: &str) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    
    let mut parts = line.split_whitespace();
    let Some(cmd) = parts.next() else { return; };
    
    match cmd {
        "help" => cmd_help(),
        "ls" => cmd_ls(),
        "touch" => {
            if let Some(arg) = parts.next() {
                cmd_touch(arg);
            } else {
                println!("usage: touch <file>");
            }
        }
        "write" => cmd_write(line),
        "cat" => {
            if let Some(arg) = parts.next() {
                cmd_cat(arg);
            } else {
                println!("usage: cat <file>");
            }
        }
        "rm" => {
            if let Some(arg) = parts.next() {
                cmd_rm(arg);
            } else {
                println!("usage: rm <file>");
            }
        }
        "mem" => {
            let verbose = parts.next() == Some("-v");
            cmd_mem(verbose);
        }
        "cpuinfo" => cmd_cpuinfo(),
        "netinfo" => cmd_netinfo(),
        "calc" => {
            if let Some(expr) = line.strip_prefix("calc").map(|s| s.trim()) {
                if !expr.is_empty() {
                    match calc::eval(expr) {
                        Ok(v) => println!("= {}", v),
                        Err(e) => println!("calc error: {}", e),
                    }
                } else {
                    println!("usage: calc <expr>");
                }
            }
        }
        "arpwhois" => cmd_arpwhois(),
        "looptest" => cmd_looptest(),
        "netdiag" => net::dump_regs(),
        "panic" => panic!("user-requested panic"),
        "spawn" => {
            if let Some(name) = parts.next() {
                cmd_spawn(name);
            } else {
                println!("usage: spawn <name>");
            }
        }
        "ps" => cmd_ps(),
        "shutdown" => cmd_shutdown(),
        "clear" => vga::clear_screen(),
        "edit" => {
            if let Some(file) = parts.next() {
                editor::run(file);
            } else {
                println!("usage: edit <file>");
            }
        }
        "hexdump" => {
            let addr_str = parts.next();
            let len_str = parts.next();
            cmd_hexdump(addr_str, len_str);
        }
        "uptime" => cmd_uptime(),
        "dhcp" => cmd_dhcp(),
        "env" => cmd_env(),
        _ => println!("unknown command: {}", cmd),
    }
}

fn cmd_help() {
    println!("help              - list commands");
    println!("ls                - list RamFS files");
    println!("touch <file>      - create empty file");
    println!("write <f> \"txt\"   - write file");
    println!("cat <file>        - print file");
    println!("rm <file>         - delete file");
    println!("mem               - heap usage");
    println!("cpuinfo           - CPU vendor");
    println!("netinfo           - NIC status");
    println!("calc <expr>       - integer math");
    println!("clear             - clear screen");
    println!("edit <file>       - TUI editor (Esc=exit, Ctrl+S=save)");
    println!("spawn <name>      - spawn test task");
    println!("ps                - list tasks");
    println!("shutdown          - power off VM");
    println!("uptime            - system uptime");
    println!("env               - system info");
    println!("dhcp              - request IP via DHCP");
}

fn cmd_ls() {
    for (name, size) in ramfs::list() {
        println!("{:16} {} bytes", name, size);
    }
}

fn cmd_touch(path: &str) {
    if !is_valid_filename(path) {
        println!("touch: invalid filename (max 255 chars, no / \\ or null bytes)");
        return;
    }
    match ramfs::create(path) {
        Ok(()) => println!("created {}", path),
        Err(e) => println!("touch: {}", e),
    }
}

fn cmd_write(line: &str) {
    let rest = line.strip_prefix("write").unwrap_or("").trim();
    let Some((path, content)) = parse_write_args(rest) else {
        println!("usage: write <file> \"content\" (filename must be valid)");
        return;
    };
    match ramfs::write(path, content.as_bytes()) {
        Ok(()) => println!("wrote {} bytes to {}", content.len(), path),
        Err(e) => println!("write: {}", e),
    }
}

fn is_valid_filename(name: &str) -> bool {
    !name.is_empty() 
        && !name.contains('/') 
        && !name.contains('\\')
        && !name.contains('\0')
        && name.len() <= 255
}

fn parse_write_args(s: &str) -> Option<(&str, String)> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    
    // Find first whitespace to split path from content
    let split_pos = s.find(char::is_whitespace)?;
    let path = s[..split_pos].trim();
    
    // Validate filename
    if !is_valid_filename(path) {
        return None;
    }
    
    let rest = s[split_pos..].trim();
    if rest.is_empty() {
        return None;
    }
    
    let mut content = rest.to_string();
    if content.starts_with('"') && content.ends_with('"') && content.len() >= 2 {
        content = content[1..content.len() - 1].to_string();
    }
    Some((path, content))
}

fn cmd_cat(path: &str) {
    if !is_valid_filename(path) {
        println!("cat: invalid filename");
        return;
    }
    match ramfs::read(path) {
        Ok(data) => {
            if let Ok(s) = core::str::from_utf8(&data) {
                println!("{}", s);
            } else {
                println!("(binary {} bytes)", data.len());
            }
        }
        Err(e) => println!("cat: {}", e),
    }
}

fn cmd_rm(path: &str) {
    if !is_valid_filename(path) {
        println!("rm: invalid filename");
        return;
    }
    match ramfs::delete(path) {
        Ok(()) => println!("removed {}", path),
        Err(e) => println!("rm: {}", e),
    }
}

fn cmd_mem(verbose: bool) {
    let (used, free) = memory::heap_stats();
    println!("heap used: {} bytes", used);
    println!("heap free: {} bytes", free);
    println!("heap total: {} bytes", memory::HEAP_SIZE);
    if verbose {
        memory::bucket_allocator::dump_stats();
    }
}

fn cmd_cpuinfo() {
    println!("vendor: {}", cpu::vendor_id());
    let brand = cpu::brand_string();
    if !brand.is_empty() {
        println!("brand:  {}", brand);
    }
}

fn local_ip() -> [u8; 4] {
    net::get_config().map(|c| c.ip).unwrap_or([10, 0, 2, 15])
}

fn cmd_arpwhois() {
    let status = net::status();
    if status.kind == NicKind::None {
        println!("No NIC available");
        return;
    }
    let our_mac = status.mac;
    let our_ip = local_ip();
    let target_ip = net::get_config().map(|c| c.gateway).unwrap_or([10, 0, 2, 2]);
    let broadcast = [0xFF; 6];

    let mut arp_buf = [0u8; 28];
    let arp_len = arp::ArpPacket::build_request(our_mac, our_ip, target_ip, &mut arp_buf)
        .expect("ARP buffer too small");

    let mut frame_buf = [0u8; 14 + 28 + 4];
    let frame_len =
        ethernet::EthernetFrame::build(broadcast, our_mac, ethernet::ETHERTYPE_ARP, &arp_buf[..arp_len], &mut frame_buf)
            .expect("frame buffer too small");

    println!("Sending ARP who-has {}.{}.{}.{} ...", target_ip[0], target_ip[1], target_ip[2], target_ip[3]);

    match net::send(&frame_buf[..frame_len]) {
        Ok(()) => println!("TX OK"),
        Err(()) => {
            println!("TX failed");
            return;
        }
    }

    // Poll for ARP reply (10 second timeout using the system timer)
    let deadline = crate::timer::millis() + 10_000;
    while crate::timer::millis() < deadline {
        if let Some(pkt) = net::poll_rx() {
            if let Some(eth) = ethernet::EthernetFrame::parse(&pkt) {
                if eth.ethertype == ethernet::ETHERTYPE_ARP {
                    if let Some(arp_pkt) = arp::ArpPacket::parse(eth.payload) {
                        if arp_pkt.opcode == 2
                            && arp_pkt.sender_ip == target_ip
                            && arp_pkt.target_ip == our_ip
                        {
                            println!(
                                "Reply from {}.{}.{}.{} is {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                                arp_pkt.sender_ip[0],
                                arp_pkt.sender_ip[1],
                                arp_pkt.sender_ip[2],
                                arp_pkt.sender_ip[3],
                                arp_pkt.sender_mac[0],
                                arp_pkt.sender_mac[1],
                                arp_pkt.sender_mac[2],
                                arp_pkt.sender_mac[3],
                                arp_pkt.sender_mac[4],
                                arp_pkt.sender_mac[5],
                            );
                            return;
                        }
                    }
                }
            }
        }

        // Check for Ctrl+C to abort
        let k = keyboard::pop_event();
        if k == Some(keyboard::KeyEvent::Ctrl('c'))
            || k == Some(keyboard::KeyEvent::Ctrl('C'))
        {
            println!("Aborted by user");
            return;
        }

        crate::arch::hlt();
    }

    // Show RX diagnostics
    let info = net::status();
    println!("No ARP reply (NIC: {:?}, MAC: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x})",
        info.kind, info.mac[0], info.mac[1], info.mac[2], info.mac[3], info.mac[4], info.mac[5]);
}

fn cmd_looptest() {
    let status = net::status();
    if status.kind == NicKind::None {
        println!("No NIC available");
        return;
    }
    let mac = status.mac;
    let ip = local_ip();
    let gw = net::get_config().map(|c| c.gateway).unwrap_or([10, 0, 2, 2]);

    // Build a minimal ARP request as test data
    let mut arp_buf = [0u8; 28];
    let arp_len = arp::ArpPacket::build_request(mac, ip, gw, &mut arp_buf).unwrap();
    let mut frame_buf = [0u8; 64];
    let frame_len = ethernet::EthernetFrame::build(
        [0xFF; 6], mac, ethernet::ETHERTYPE_ARP, &arp_buf[..arp_len], &mut frame_buf,
    ).unwrap();

    // Enable loopback
    net::set_loopback(true);
    println!("Loopback ON, sending packet...");
    let _ = net::send(&frame_buf[..frame_len]);
    println!("TX done, checking RX...");

    // Poll a few times for the looped-back packet
    let deadline = crate::timer::millis() + 500;
    let mut got = false;
    while crate::timer::millis() < deadline {
        if let Some(pkt) = net::poll_rx() {
            got = true;
            println!("LOOPBACK OK: received {} bytes!", pkt.len());
            if let Some(eth) = ethernet::EthernetFrame::parse(&pkt) {
                println!("  dst={:02x}:{:02x}:... src={:02x}:{:02x}:... ethertype=0x{:04x}",
                    eth.dst[0], eth.dst[1], eth.src[0], eth.src[1], eth.ethertype);
            }
            break;
        }
        crate::arch::hlt();
    }

    net::set_loopback(false);
    println!("Loopback OFF");

    if !got {
        println!("No RX data — RX path broken");
        let (capr, cbr) = net::rx_ring_pos();
        println!("  CAPR={} CBR={}", capr, cbr);
    }
}

/// A test task — just loops forever with hlt to save CPU.
fn test_task() {
    loop {
        crate::arch::hlt();
    }
}

fn cmd_spawn(_name: &str) {
    let id = crate::scheduler::spawn(test_task);
    if id >= 0 {
        println!("Task {} spawned", id);
    } else {
        println!("No free task slot");
    }
}

fn cmd_ps() {
    crate::scheduler::print_tasks();
}

fn cmd_shutdown() {
    println!("Shutting down...");
    // QEMU-специфичный порт отключения питания
    unsafe { x86::io::outw(0x604, 0x2000); }
    // Если не сработало — зависаем
    loop {
        crate::arch::hlt();
    }
}

fn parse_hex(s: &str) -> Option<u32> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    u32::from_str_radix(s, 16).ok()
}

fn cmd_uptime() {
    let (secs, ms) = crate::timer::elapsed();
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    println!("uptime: {:02}:{:02}:{:02}.{:03}", h, m, s, ms);
}

fn cmd_hexdump(addr_str: Option<&str>, len_str: Option<&str>) {
    let addr = addr_str.and_then(parse_hex).unwrap_or(0);
    let len = len_str.and_then(|s| s.parse::<usize>().ok()).unwrap_or(128);
    if len == 0 || len > 4096 {
        println!("invalid length (max 4096)");
        return;
    }
    let ptr = addr as *const u8;
    for off in (0..len).step_by(16) {
        let line_start = addr + off as u32;
        print!("{:08x}  ", line_start);
        let remaining = len - off;
        let line_len = remaining.min(16);
        for i in 0..16 {
            if i < line_len {
                let b = unsafe { *ptr.add(off + i) };
                print!("{:02x} ", b);
            } else {
                print!("   ");
            }
            if i == 7 {
                print!(" ");
            }
        }
        print!(" |");
        for i in 0..line_len {
            let b = unsafe { *ptr.add(off + i) };
            if b.is_ascii_graphic() || b == b' ' {
                print!("{}", b as char);
            } else {
                print!(".");
            }
        }
        println!("|");
    }
}

fn cmd_dhcp() {
    let status = net::status();
    if status.kind == NicKind::None {
        println!("No NIC available");
        return;
    }
    let xid = crate::timer::millis() as u32;
    println!("DHCP discover (xid=0x{:08x})...", xid);
    match net::dhcp::dhcp_request(&status.mac, xid) {
        Some(cfg) => {
            net::set_config(cfg);
            println!("DHCP OK!");
            cfg.display();
        }
        None => {
            println!("DHCP failed (no response or bad ack)");
        }
    }
}

fn cmd_env() {
    // Heap
    let (used, free) = crate::memory::heap_stats();
    let total = used + free;
    println!("heap: {}/{} bytes used ({:.1}%)", used, total, 100.0 * used as f64 / total as f64);

    // CPU
    let vendor = crate::cpu::vendor_id();
    println!("cpu: {}", vendor);

    // Page allocator
    let (total, free, used) = crate::memory::paging::stats();
    println!("pages: {} total, {} free, {} used ({} KB)", total, free, used, total * 4);

    // Uptime
    let (secs, ms) = crate::timer::elapsed();
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    println!("uptime: {:02}:{:02}:{:02}.{:03}", h, m, s, ms);
}

fn cmd_netinfo() {
    let s = net::status();
    let kind = match s.kind {
        NicKind::None => "none (stub)",
        NicKind::E1000 => "Intel e1000 (skeleton)",
        NicKind::Rtl8139 => "Realtek RTL8139",
    };
    println!("interface: {}", kind);
    println!(
        "mac: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        s.mac[0], s.mac[1], s.mac[2], s.mac[3], s.mac[4], s.mac[5]
    );
    println!("link: {}", if s.link_up { "up" } else { "down" });
    if s.kind == NicKind::Rtl8139 {
        let (capr, cbr) = net::rx_ring_pos();
        println!("rx ring: CAPR={} CBR={}", capr, cbr);
    }
    if let Some(p) = s.pci {
        println!(
            "pci {:02x}:{:02x}.0 - vendor {:04x} device {:04x}",
            p.bus, p.slot, p.vendor_id, p.device_id
        );
    }
}
