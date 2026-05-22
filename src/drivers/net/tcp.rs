//! Minimal TCP implementation for the kernel.
//!
//! Features:
//! - Three-way handshake (active open: connect; passive open: listen)
//! - Data send/receive with sequence numbers
//! - Four-way close (FIN handshake)
//! - Retransmission timer (1s timeout, 3 retries)
//! - Fixed window (8192), MSS=536
//! - Poll-based: call tcp_poll() regularly (integrated into receive path)
//!
//! Limitations:
//! - No congestion control, no Nagle, no delayed ACK
//! - Fixed connection table (4 slots)
//! - No urgent pointer, no TCP options beyond MSS during SYN
//! - No window scaling, no SACK

use crate::timer;
use crate::{info, warn, debug};

// ── Constants ────────────────────────────────────────────────────────────────

pub const TCP_PROTO: u8 = 6;

/// Maximum segment size (safe default for IPv4).
pub const MSS: usize = 536;

/// Receive window we advertise.
const WINDOW: u16 = 8192;

/// Retransmission timeout in milliseconds.
const RTO_MS: u64 = 1000;

/// Maximum retransmissions before giving up.
const MAX_RETRIES: u8 = 3;

/// Max simultaneous connections.
const MAX_CONNS: usize = 4;

// ── TCP flags ────────────────────────────────────────────────────────────────

const FIN: u8 = 0x01;
const SYN: u8 = 0x02;
const RST: u8 = 0x04;
const PSH: u8 = 0x08;
const ACK: u8 = 0x10;

// ── TCP state machine ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcpState {
    Closed,
    Listen,
    SynSent,
    SynReceived,
    Established,
    FinWait1,
    FinWait2,
    CloseWait,
    LastAck,
    TimeWait,
}

// ── TCP header ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TcpHeader<'a> {
    pub src_port: u16,
    pub dst_port: u16,
    pub seq: u32,
    pub ack: u32,
    pub data_offset: u8,
    pub flags: u8,
    pub window: u16,
    pub checksum: u16,
    pub urgent: u16,
    pub payload: &'a [u8],
}

impl<'a> TcpHeader<'a> {
    /// Parse a TCP segment. `data` starts at the TCP header.
    pub fn parse(data: &'a [u8]) -> Option<Self> {
        if data.len() < 20 {
            return None;
        }
        let src_port = u16::from_be_bytes([data[0], data[1]]);
        let dst_port = u16::from_be_bytes([data[2], data[3]]);
        let seq = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        let ack = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
        let data_offset = (data[12] >> 4) * 4;
        if data_offset < 20 || data_offset as usize > data.len() {
            return None;
        }
        let flags = data[13] & 0x3F;
        let window = u16::from_be_bytes([data[14], data[15]]);
        let checksum = u16::from_be_bytes([data[16], data[17]]);
        let urgent = u16::from_be_bytes([data[18], data[19]]);
        Some(TcpHeader {
            src_port,
            dst_port,
            seq,
            ack,
            data_offset,
            flags,
            window,
            checksum,
            urgent,
            payload: &data[data_offset as usize..],
        })
    }

    /// Build a TCP segment into `out`. Returns total bytes written.
    /// Checksum placeholder is filled after construction via `tcp_checksum`.
    pub fn build(
        src_port: u16,
        dst_port: u16,
        seq: u32,
        ack: u32,
        flags: u8,
        window: u16,
        src_ip: [u8; 4],
        dst_ip: [u8; 4],
        payload: &[u8],
        out: &mut [u8],
    ) -> Option<usize> {
        let hdr_len: usize = 20;
        let total = hdr_len + payload.len();
        if out.len() < total {
            return None;
        }

        out[0..2].copy_from_slice(&src_port.to_be_bytes());
        out[2..4].copy_from_slice(&dst_port.to_be_bytes());
        out[4..8].copy_from_slice(&seq.to_be_bytes());
        out[8..12].copy_from_slice(&ack.to_be_bytes());
        out[12] = ((hdr_len / 4) as u8) << 4;
        out[13] = flags;
        out[14..16].copy_from_slice(&window.to_be_bytes());
        out[16..18].copy_from_slice(&[0u8; 2]);
        out[18..20].copy_from_slice(&[0u8; 2]);
        out[20..total].copy_from_slice(payload);

        let cksum = tcp_checksum(src_ip, dst_ip, &out[..total]);
        out[16..18].copy_from_slice(&cksum.to_be_bytes());

        Some(total)
    }
}

// ── Connection tracking ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct TcpConn {
    state: TcpState,
    local_port: u16,
    remote_ip: [u8; 4],
    remote_port: u16,

    local_seq: u32,
    remote_seq: u32,
    local_ack: u32,

    send_buf: [u8; 1024],
    send_len: usize,
    send_acked: usize,

    recv_buf: [u8; 4096],
    recv_len: usize,

    last_tx_time: u64,
    last_tx_seq: u32,
    last_tx_len: usize,
    last_tx_flags: u8,
    last_tx_payload: [u8; MSS],
    retries: u8,
    rto_ms: u64,
}

impl TcpConn {
    const fn new() -> Self {
        Self {
            state: TcpState::Closed,
            local_port: 0,
            remote_ip: [0; 4],
            remote_port: 0,
            local_seq: 0,
            remote_seq: 0,
            local_ack: 0,
            send_buf: [0; 1024],
            send_len: 0,
            send_acked: 0,
            recv_buf: [0; 4096],
            recv_len: 0,
            last_tx_time: 0,
            last_tx_seq: 0,
            last_tx_len: 0,
            last_tx_flags: 0,
            last_tx_payload: [0; MSS],
            retries: 0,
            rto_ms: RTO_MS,
        }
    }

    fn closed(&self) -> bool {
        matches!(self.state, TcpState::Closed | TcpState::TimeWait)
    }
}

// ── Global connection table ──────────────────────────────────────────────────

use spin::Mutex;

static CONNS: Mutex<[TcpConn; MAX_CONNS]> = Mutex::new([
    TcpConn::new(),
    TcpConn::new(),
    TcpConn::new(),
    TcpConn::new(),
]);

// ── Sequence number helpers ──────────────────────────────────────────────────

fn isn() -> u32 {
    timer::millis() as u32
}

fn seq_lt(a: u32, b: u32) -> bool {
    (b.wrapping_sub(a)) < 0x8000_0000
}

#[allow(dead_code)]
fn seq_le(a: u32, b: u32) -> bool {
    a == b || seq_lt(a, b)
}

// ── Checksum ─────────────────────────────────────────────────────────────────

fn tcp_checksum(src_ip: [u8; 4], dst_ip: [u8; 4], segment: &[u8]) -> u16 {
    let mut sum: u32 = 0;

    sum += u16::from_be_bytes([src_ip[0], src_ip[1]]) as u32;
    sum += u16::from_be_bytes([src_ip[2], src_ip[3]]) as u32;
    sum += u16::from_be_bytes([dst_ip[0], dst_ip[1]]) as u32;
    sum += u16::from_be_bytes([dst_ip[2], dst_ip[3]]) as u32;
    sum += 6u32;
    sum += segment.len() as u32;

    let mut i = 0;
    while i + 1 < segment.len() {
        sum += u16::from_be_bytes([segment[i], segment[i + 1]]) as u32;
        i += 2;
    }
    if i < segment.len() {
        sum += (segment[i] as u32) << 8;
    }

    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

// ── Public API ───────────────────────────────────────────────────────────────

pub fn listen(port: u16) -> Result<(), &'static str> {
    let mut conns = CONNS.lock();
    for conn in conns.iter_mut() {
        if conn.state == TcpState::Closed {
            conn.state = TcpState::Listen;
            conn.local_port = port;
            conn.remote_ip = [0; 4];
            conn.remote_port = 0;
            conn.local_seq = isn();
            conn.remote_seq = 0;
            conn.local_ack = 0;
            info!("tcp: listening on port {}", port);
            return Ok(());
        }
    }
    Err("no free connection slots")
}

pub fn connect(local_port: u16, remote_ip: [u8; 4], remote_port: u16) -> Result<usize, &'static str> {
    let mut conns = CONNS.lock();
    for (i, conn) in conns.iter_mut().enumerate() {
        if conn.state == TcpState::Closed {
            conn.state = TcpState::SynSent;
            conn.local_port = local_port;
            conn.remote_ip = remote_ip;
            conn.remote_port = remote_port;
            conn.local_seq = isn();
            conn.remote_seq = 0;
            conn.local_ack = 0;
            conn.retries = 0;
            conn.rto_ms = RTO_MS;
            send_segment(conn, SYN, &[]);
            info!("tcp: SYN sent to {}.{}.{}.{}:{} (port {})",
                remote_ip[0], remote_ip[1], remote_ip[2], remote_ip[3],
                remote_port, local_port);
            return Ok(i);
        }
    }
    Err("no free connection slots")
}

pub fn send(idx: usize, data: &[u8]) -> Result<usize, &'static str> {
    let mut conns = CONNS.lock();
    let conn = conns.get_mut(idx).ok_or("invalid connection index")?;
    if conn.state != TcpState::Established && conn.state != TcpState::CloseWait {
        return Err("connection not established");
    }
    let to_copy = data.len().min(conn.send_buf.len() - conn.send_len);
    if to_copy == 0 {
        return Err("send buffer full");
    }
    conn.send_buf[conn.send_len..conn.send_len + to_copy].copy_from_slice(&data[..to_copy]);
    conn.send_len += to_copy;
    flush_send(conn);
    Ok(to_copy)
}

pub fn recv(idx: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
    let mut conns = CONNS.lock();
    let conn = conns.get_mut(idx).ok_or("invalid connection index")?;
    if conn.recv_len == 0 {
        return Ok(0);
    }
    let n = conn.recv_len.min(buf.len());
    buf[..n].copy_from_slice(&conn.recv_buf[..n]);
    if n < conn.recv_len {
        conn.recv_buf.copy_within(n..conn.recv_len, 0);
    }
    conn.recv_len -= n;
    Ok(n)
}

pub fn close(idx: usize) -> Result<(), &'static str> {
    let mut conns = CONNS.lock();
    let conn = conns.get_mut(idx).ok_or("invalid connection index")?;
    match conn.state {
        TcpState::Established => {
            flush_send(conn);
            conn.state = TcpState::FinWait1;
            send_segment(conn, FIN | ACK, &[]);
            info!("tcp: FIN sent (active close)");
        }
        TcpState::CloseWait => {
            conn.state = TcpState::LastAck;
            send_segment(conn, FIN | ACK, &[]);
            info!("tcp: FIN sent (passive close)");
        }
        _ => return Err("connection not in closable state"),
    }
    Ok(())
}

#[allow(dead_code)]
pub fn status(idx: usize) -> Result<(TcpState, u16, [u8; 4], u16, usize), &'static str> {
    let conns = CONNS.lock();
    let conn = conns.get(idx).ok_or("invalid connection index")?;
    Ok((conn.state, conn.local_port, conn.remote_ip, conn.remote_port, conn.recv_len))
}

pub fn list() -> alloc::vec::Vec<(usize, TcpState, u16, [u8; 4], u16)> {
    let conns = CONNS.lock();
    let mut v = alloc::vec::Vec::new();
    for (i, conn) in conns.iter().enumerate() {
        if !conn.closed() {
            v.push((i, conn.state, conn.local_port, conn.remote_ip, conn.remote_port));
        }
    }
    v
}

// ── Retransmission ──────────────────────────────────────────────────────────

fn check_retransmit(conn: &mut TcpConn) {
    if conn.retries >= MAX_RETRIES {
        warn!("tcp: max retries on port {} — resetting", conn.local_port);
        send_segment_raw(conn, RST | ACK, &[]);
        *conn = TcpConn::new();
        return;
    }
    let now = timer::millis();
    if now - conn.last_tx_time >= conn.rto_ms {
        debug!("tcp: retransmit #{} (seq={}, len={})", conn.retries + 1, conn.last_tx_seq, conn.last_tx_len);
        let plen = conn.last_tx_len;
        let mut payload_buf = [0u8; MSS];
        payload_buf[..plen].copy_from_slice(&conn.last_tx_payload[..plen]);
        let flags = conn.last_tx_flags;
        send_segment_raw(conn, flags, &payload_buf[..plen]);
        conn.retries += 1;
        conn.rto_ms *= 2;
        conn.last_tx_time = timer::millis();
    }
}

// ── Internal send ────────────────────────────────────────────────────────────

fn send_segment_raw(conn: &mut TcpConn, flags: u8, payload: &[u8]) {
    let mut tcp_buf = [0u8; 20 + MSS];
    let cfg = super::get_config();
    let src_ip = match cfg {
        Some(c) => c.ip,
        None => return,
    };

    let tcp_len = match TcpHeader::build(
        conn.local_port,
        conn.remote_port,
        conn.local_seq,
        conn.local_ack,
        flags,
        WINDOW,
        src_ip,
        conn.remote_ip,
        payload,
        &mut tcp_buf,
    ) {
        Some(l) => l,
        None => return,
    };

    if flags & (SYN | FIN) != 0 || !payload.is_empty() {
        conn.last_tx_time = timer::millis();
        conn.last_tx_seq = conn.local_seq;
        conn.last_tx_len = payload.len();
        conn.last_tx_flags = flags;
        let copy_len = payload.len().min(MSS);
        conn.last_tx_payload[..copy_len].copy_from_slice(&payload[..copy_len]);
    }

    let mut ip_buf = [0u8; 20 + 20 + MSS];
    let ip_hdr_len = match super::ipv4::Ipv4Packet::build(
        src_ip,
        conn.remote_ip,
        TCP_PROTO,
        tcp_len as u16,
        &mut ip_buf,
    ) {
        Some(l) => l,
        None => return,
    };
    ip_buf[ip_hdr_len..ip_hdr_len + tcp_len].copy_from_slice(&tcp_buf[..tcp_len]);

    let cfg = super::get_config();
    let dst_ip = cfg.map(|c| c.gateway).unwrap_or([10, 0, 2, 2]);
    let dst_mac = resolve_mac(dst_ip);
    let mac = super::status().mac;
    let ip_total = ip_hdr_len + tcp_len;
    let mut eth_buf = [0u8; 14 + 20 + 20 + MSS];
    let eth_len = match super::ethernet::EthernetFrame::build(
        dst_mac,
        mac,
        0x0800,
        &ip_buf[..ip_total],
        &mut eth_buf,
    ) {
        Some(l) => l,
        None => return,
    };

    let _ = super::send(&eth_buf[..eth_len]);
}

fn send_segment(conn: &mut TcpConn, flags: u8, payload: &[u8]) {
    let consumed = if flags & SYN != 0 { 1 } else { 0 }
        + if flags & FIN != 0 { 1 } else { 0 }
        + payload.len() as u32;
    conn.local_seq = conn.local_seq.wrapping_add(consumed);
    send_segment_raw(conn, flags, payload);
}

fn flush_send(conn: &mut TcpConn) {
    if conn.send_len == 0 {
        return;
    }
    let to_send = conn.send_len.min(MSS);
    let mut payload_buf = [0u8; MSS];
    payload_buf[..to_send].copy_from_slice(&conn.send_buf[..to_send]);
    send_segment(conn, PSH | ACK, &payload_buf[..to_send]);
}

// ── ARP helper ───────────────────────────────────────────────────────────────

fn resolve_mac(ip: [u8; 4]) -> [u8; 6] {
    use super::arp::ArpPacket;
    let mac = super::status().mac;
    let mut arp_buf = [0u8; 28];
    if let Some(len) = ArpPacket::build_request(mac, [10, 0, 2, 15], ip, &mut arp_buf) {
        let mut eth_buf = [0u8; 14 + 28];
        if let Some(eth_len) = super::ethernet::EthernetFrame::build(
            [0xFF; 6], mac, 0x0806, &arp_buf[..len], &mut eth_buf,
        ) {
            let _ = super::send(&eth_buf[..eth_len]);
        }
    }
    [0xFF; 6]
}

// ── Receive path ─────────────────────────────────────────────────────────────

pub fn receive(src_ip: [u8; 4], _dst_ip: [u8; 4], data: &[u8]) {
    let hdr = match TcpHeader::parse(data) {
        Some(h) => h,
        None => return,
    };

    let mut conns = CONNS.lock();

    let conn_idx = conns.iter().position(|c| {
        c.state != TcpState::Closed
            && c.state != TcpState::TimeWait
            && c.local_port == hdr.dst_port
            && (c.remote_ip == [0; 4] || c.remote_ip == src_ip)
            && (c.remote_port == 0 || c.remote_port == hdr.src_port)
    });

    let conn = match conn_idx {
        Some(i) => &mut conns[i],
        None => return,
    };

    if conn.remote_ip == [0; 4] {
        conn.remote_ip = src_ip;
    }
    if conn.remote_port == 0 {
        conn.remote_port = hdr.src_port;
    }

    if hdr.flags & RST != 0 {
        warn!("tcp: RST received on port {} — closing", conn.local_port);
        *conn = TcpConn::new();
        return;
    }

    process_segment(conn, &hdr);
}

// ── Segment processing ───────────────────────────────────────────────────────

fn process_segment(conn: &mut TcpConn, hdr: &TcpHeader) {
    match conn.state {
        TcpState::Listen => handle_listen(conn, hdr),
        TcpState::SynSent => handle_syn_sent(conn, hdr),
        TcpState::SynReceived => handle_syn_received(conn, hdr),
        TcpState::Established => handle_established(conn, hdr),
        TcpState::FinWait1 => handle_fin_wait1(conn, hdr),
        TcpState::FinWait2 => handle_fin_wait2(conn, hdr),
        TcpState::CloseWait => handle_close_wait(conn, hdr),
        TcpState::LastAck => handle_last_ack(conn, hdr),
        _ => {}
    }
}

fn handle_listen(conn: &mut TcpConn, hdr: &TcpHeader) {
    if hdr.flags & SYN == 0 {
        return;
    }
    conn.remote_seq = hdr.seq.wrapping_add(1);
    conn.local_ack = hdr.seq.wrapping_add(1);
    conn.state = TcpState::SynReceived;
    send_segment(conn, SYN | ACK, &[]);
    info!("tcp: SYN received on port {} — SYN+ACK sent", conn.local_port);
}

fn handle_syn_sent(conn: &mut TcpConn, hdr: &TcpHeader) {
    if hdr.flags & (SYN | ACK) != (SYN | ACK) {
        return;
    }
    if hdr.ack != conn.local_seq {
        debug!("tcp: SYN+ACK wrong ack (expected {}, got {})", conn.local_seq, hdr.ack);
        return;
    }
    conn.remote_seq = hdr.seq.wrapping_add(1);
    conn.local_ack = hdr.seq.wrapping_add(1);
    conn.state = TcpState::Established;
    conn.retries = 0;
    conn.rto_ms = RTO_MS;
    send_segment_raw(conn, ACK, &[]);
    info!("tcp: established to {}.{}.{}.{}:{}",
        conn.remote_ip[0], conn.remote_ip[1], conn.remote_ip[2], conn.remote_ip[3],
        conn.remote_port);
    flush_send(conn);
}

fn handle_syn_received(conn: &mut TcpConn, hdr: &TcpHeader) {
    if hdr.flags & ACK == 0 {
        return;
    }
    if hdr.ack != conn.local_seq {
        debug!("tcp: handshake ACK wrong ack (expected {}, got {})", conn.local_seq, hdr.ack);
        return;
    }
    conn.state = TcpState::Established;
    conn.retries = 0;
    conn.rto_ms = RTO_MS;
    info!("tcp: established on port {} from {}.{}.{}.{}:{}",
        conn.local_port,
        conn.remote_ip[0], conn.remote_ip[1], conn.remote_ip[2], conn.remote_ip[3],
        conn.remote_port);
}

fn handle_established(conn: &mut TcpConn, hdr: &TcpHeader) {
    if hdr.flags & ACK != 0 {
        let ack = hdr.ack;
        let acked_start = conn.local_seq.wrapping_sub(conn.send_len as u32);
        if seq_lt(acked_start, ack) || ack == conn.local_seq {
            conn.retries = 0;
            conn.rto_ms = RTO_MS;
            // Advance acked data
            let mut acked = ack.wrapping_sub(acked_start);
            if acked > conn.send_len as u32 {
                acked = conn.send_len as u32;
            }
            if acked > 0 {
                let a = acked as usize;
                conn.send_buf.copy_within(a..conn.send_len, 0);
                conn.send_len -= a;
            }
            conn.send_acked = ack as usize;
            flush_send(conn);
        }
    }

    if !hdr.payload.is_empty() {
        if hdr.seq != conn.remote_seq {
            send_segment_raw(conn, ACK, &[]);
            return;
        }
        let copy_len = hdr.payload.len().min(conn.recv_buf.len() - conn.recv_len);
        conn.recv_buf[conn.recv_len..conn.recv_len + copy_len]
            .copy_from_slice(&hdr.payload[..copy_len]);
        conn.recv_len += copy_len;
        conn.remote_seq = conn.remote_seq.wrapping_add(hdr.payload.len() as u32);
        conn.local_ack = conn.remote_seq;
        send_segment_raw(conn, ACK, &[]);
    }

    if hdr.flags & FIN != 0 {
        conn.remote_seq = conn.remote_seq.wrapping_add(1);
        conn.local_ack = conn.remote_seq;
        send_segment_raw(conn, ACK, &[]);
        conn.state = TcpState::CloseWait;
        info!("tcp: FIN received on port {} — CLOSE_WAIT", conn.local_port);
    }
}

fn handle_fin_wait1(conn: &mut TcpConn, hdr: &TcpHeader) {
    if hdr.flags & ACK != 0 {
        if hdr.flags & FIN != 0 {
            conn.remote_seq = conn.remote_seq.wrapping_add(1);
            conn.local_ack = conn.remote_seq;
            send_segment_raw(conn, ACK, &[]);
            conn.state = TcpState::TimeWait;
            conn.last_tx_time = timer::millis();
            info!("tcp: closed (TIME_WAIT)");
        } else {
            conn.state = TcpState::FinWait2;
        }
    }
    if !hdr.payload.is_empty() && hdr.seq == conn.remote_seq {
        let n = hdr.payload.len().min(conn.recv_buf.len() - conn.recv_len);
        conn.recv_buf[conn.recv_len..conn.recv_len + n].copy_from_slice(&hdr.payload[..n]);
        conn.recv_len += n;
        conn.remote_seq = conn.remote_seq.wrapping_add(hdr.payload.len() as u32);
    }
}

fn handle_fin_wait2(conn: &mut TcpConn, hdr: &TcpHeader) {
    if hdr.flags & FIN != 0 {
        conn.remote_seq = conn.remote_seq.wrapping_add(1);
        conn.local_ack = conn.remote_seq;
        send_segment_raw(conn, ACK, &[]);
        conn.state = TcpState::TimeWait;
        conn.last_tx_time = timer::millis();
        info!("tcp: closed (TIME_WAIT)");
    }
    if !hdr.payload.is_empty() && hdr.seq == conn.remote_seq {
        let n = hdr.payload.len().min(conn.recv_buf.len() - conn.recv_len);
        conn.recv_buf[conn.recv_len..conn.recv_len + n].copy_from_slice(&hdr.payload[..n]);
        conn.recv_len += n;
        conn.remote_seq = conn.remote_seq.wrapping_add(hdr.payload.len() as u32);
    }
}

fn handle_close_wait(conn: &mut TcpConn, hdr: &TcpHeader) {
    if !hdr.payload.is_empty() && hdr.seq == conn.remote_seq {
        let n = hdr.payload.len().min(conn.recv_buf.len() - conn.recv_len);
        conn.recv_buf[conn.recv_len..conn.recv_len + n].copy_from_slice(&hdr.payload[..n]);
        conn.recv_len += n;
        conn.remote_seq = conn.remote_seq.wrapping_add(hdr.payload.len() as u32);
        conn.local_ack = conn.remote_seq;
        send_segment_raw(conn, ACK, &[]);
    }
    if hdr.flags & FIN != 0 {
        conn.local_ack = conn.remote_seq;
        send_segment_raw(conn, ACK, &[]);
    }
}

fn handle_last_ack(conn: &mut TcpConn, hdr: &TcpHeader) {
    if hdr.flags & ACK != 0 {
        conn.state = TcpState::Closed;
        info!("tcp: closed");
    }
}

// ── Periodic poll ────────────────────────────────────────────────────────────

pub fn poll() {
    let now = timer::millis();
    let mut conns = CONNS.lock();
    for conn in conns.iter_mut() {
        match conn.state {
            TcpState::SynSent | TcpState::SynReceived => {
                check_retransmit(conn);
            }
            TcpState::Established | TcpState::CloseWait => {
                if conn.send_len > 0 {
                    flush_send(conn);
                }
            }
            TcpState::FinWait1 | TcpState::LastAck => {
                check_retransmit(conn);
            }
            TcpState::TimeWait => {
                if now - conn.last_tx_time >= 60_000 {
                    conn.state = TcpState::Closed;
                }
            }
            _ => {}
        }
    }
}
