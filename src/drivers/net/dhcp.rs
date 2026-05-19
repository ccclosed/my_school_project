use alloc::vec::Vec;

use super::ethernet::{EthernetFrame, ETHERTYPE_IPV4};
use super::ipv4::Ipv4Packet;
use super::udp::UdpHeader;
use super::NetConfig;

const DHCP_SERVER: u16 = 67;
const DHCP_CLIENT: u16 = 68;

const BROADCAST: [u8; 4] = [255, 255, 255, 255];
const BROADCAST_MAC: [u8; 6] = [0xFF; 6];

const OP_BOOTREQUEST: u8 = 1;
const OP_BOOTREPLY: u8 = 2;

const DHCP_DISCOVER: u8 = 1;
const DHCP_REQUEST: u8 = 3;
const DHCP_ACK: u8 = 5;

/// Standard DHCP magic cookie.
const MAGIC_COOKIE: [u8; 4] = [0x63, 0x82, 0x53, 0x63];

const DHCP_MSG_SIZE: usize = 240;

const DHCP_TIMEOUT_MS: u64 = 5000;

fn build_dhcp_msg(op: u8, xid: u32, mac: &[u8; 6], msg_type: u8, req_ip: Option<[u8; 4]>, server_id: Option<[u8; 4]>) -> Vec<u8> {
    let mut buf = Vec::with_capacity(300);
    buf.push(op);                       // op
    buf.push(1);                        // htype (Ethernet)
    buf.push(6);                        // hlen
    buf.push(0);                        // hops
    buf.extend_from_slice(&xid.to_be_bytes());
    buf.extend_from_slice(&[0, 0]);     // secs
    buf.extend_from_slice(&[0x80, 0]);  // flags (broadcast)
    buf.extend_from_slice(&[0; 4]);     // ciaddr
    buf.extend_from_slice(&[0; 4]);     // yiaddr
    buf.extend_from_slice(&[0; 4]);     // siaddr
    buf.extend_from_slice(&[0; 4]);     // giaddr
    buf.extend_from_slice(&mac[..]);    // chaddr (16 bytes)
    buf.extend_from_slice(&[0; 10]);    // chaddr padding to 16
    buf.extend_from_slice(&[0; 64]);    // sname
    buf.extend_from_slice(&[0; 128]);   // file

    // Options
    buf.extend_from_slice(&MAGIC_COOKIE);

    // Option 53: DHCP message type
    buf.push(53); buf.push(1); buf.push(msg_type);

    if let Some(ip) = req_ip {
        buf.push(50); buf.push(4); buf.extend_from_slice(&ip);
    }

    if let Some(srv) = server_id {
        buf.push(54); buf.push(4); buf.extend_from_slice(&srv);
    }

    // Option 55: Parameter request list
    buf.push(55); buf.push(3);
    buf.push(1);   // subnet mask
    buf.push(3);   // router (gateway)
    buf.push(6);   // DNS

    // Option 12: Hostname (minimal)
    buf.push(12); buf.push(2);
    buf.extend_from_slice(b"os");

    // End
    buf.push(255);

    // Pad to 240+ bytes (some servers require minimum)
    while buf.len() < DHCP_MSG_SIZE {
        buf.push(0);
    }

    buf
}

/// Parse DHCP TLV options.
/// Returns (msg_type, subnet, gateway, dns, server_id).
fn parse_dhcp_options(opts: &[u8]) -> (u8, Option<[u8; 4]>, Option<[u8; 4]>, Option<[u8; 4]>, Option<[u8; 4]>) {
    let mut msg_type = 0;
    let mut subnet = None;
    let mut gateway = None;
    let mut dns = None;
    let mut server_id = None;

    let mut i = 0;
    while i < opts.len() {
        match opts[i] {
            0 => { i += 1; }                     // pad
            255 => { break; }                     // end
            tag => {
                if i + 1 >= opts.len() { break; }
                let len = opts[i + 1] as usize;
                if i + 2 + len > opts.len() { break; }
                match tag {
                    53 if len == 1 => msg_type = opts[i + 2],
                    54 if len == 4 => {
                        let mut s = [0u8; 4];
                        s.copy_from_slice(&opts[i + 2..i + 6]);
                        server_id = Some(s);
                    }
                    1 if len == 4 => {
                        let mut s = [0u8; 4];
                        s.copy_from_slice(&opts[i + 2..i + 6]);
                        subnet = Some(s);
                    }
                    3 if len == 4 => {
                        let mut s = [0u8; 4];
                        s.copy_from_slice(&opts[i + 2..i + 6]);
                        gateway = Some(s);
                    }
                    6 if len >= 4 => {
                        let mut s = [0u8; 4];
                        s.copy_from_slice(&opts[i + 2..i + 6]);
                        dns = Some(s);
                    }
                    _ => {}
                }
                i += 2 + len;
            }
        }
    }
    (msg_type, subnet, gateway, dns, server_id)
}

/// Parse a DHCP reply packet.
/// Returns (msg_type, yiaddr, subnet, gateway, dns, server_id).
fn parse_dhcp_reply(data: &[u8], xid: u32, mac: &[u8; 6]) -> Option<(u8, Option<[u8; 4]>, Option<[u8; 4]>, Option<[u8; 4]>, Option<[u8; 4]>, Option<[u8; 4]>)> {
    if data.len() < DHCP_MSG_SIZE { return None; }
    if data[0] != OP_BOOTREPLY { return None; }

    let pkt_xid = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    if pkt_xid != xid { return None; }

    // Check chaddr matches
    let mut pkt_mac = [0u8; 6];
    pkt_mac.copy_from_slice(&data[28..34]);
    if pkt_mac != *mac { return None; }

    let mut yiaddr = [0u8; 4];
    yiaddr.copy_from_slice(&data[16..20]);
    let has_yiaddr = yiaddr != [0, 0, 0, 0];
    let yiaddr = if has_yiaddr { Some(yiaddr) } else { None };

    // Parse options starting after magic cookie
    let mut opt_start = DHCP_MSG_SIZE;
    for i in DHCP_MSG_SIZE..data.len() {
        if i + 3 <= data.len() && data[i..i + 4] == MAGIC_COOKIE {
            opt_start = i + 4;
            break;
        }
    }

    let (msg_type, subnet, gateway, dns, server_id) = parse_dhcp_options(&data[opt_start..]);
    Some((msg_type, yiaddr, subnet, gateway, dns, server_id))
}

/// Run a full DHCP transaction (discover → offer → request → ack).
/// Returns NetConfig on success.
pub fn dhcp_request(mac: &[u8; 6], xid: u32) -> Option<NetConfig> {
    let discover = build_dhcp_msg(OP_BOOTREQUEST, xid, mac, DHCP_DISCOVER, None, None);
    let offered = send_and_recv(&discover, mac, xid)?;
    let (_, yiaddr, _subnet, _gateway, _dns, server_id) = offered;
    let offered_ip = yiaddr?;

    let request = build_dhcp_msg(OP_BOOTREQUEST, xid, mac, DHCP_REQUEST, Some(offered_ip), server_id);
    let acked = send_and_recv(&request, mac, xid)?;
    let (msg_type, _, subnet, gateway, dns, _) = acked;
    if msg_type != DHCP_ACK {
        return None;
    }

    Some(NetConfig {
        ip: offered_ip,
        subnet: subnet.unwrap_or([255, 255, 255, 0]),
        gateway: gateway.unwrap_or([10, 0, 2, 2]),
        dns: dns.unwrap_or([8, 8, 8, 8]),
    })
}

/// Send a DHCP message and poll for the response with a timer-based timeout.
fn send_and_recv(payload: &[u8], mac: &[u8; 6], xid: u32) -> Option<(u8, Option<[u8; 4]>, Option<[u8; 4]>, Option<[u8; 4]>, Option<[u8; 4]>, Option<[u8; 4]>)> {
    let local_mac = *mac;

    // Build UDP
    let mut udp_buf = [0u8; 512];
    let udp_len = UdpHeader::build(DHCP_CLIENT, DHCP_SERVER, payload, &mut udp_buf)?;

    // Build IP (0.0.0.0 → 255.255.255.255)
    let mut ip_buf = [0u8; 532];
    let ip_hdr_len = Ipv4Packet::build([0; 4], BROADCAST, 17, udp_len as u16, &mut ip_buf)?;
    ip_buf[ip_hdr_len..ip_hdr_len + udp_len].copy_from_slice(&udp_buf[..udp_len]);

    // Build Ethernet
    let ip_total = ip_hdr_len + udp_len;
    let mut eth_buf = [0u8; 14 + 532];
    let eth_len = EthernetFrame::build(BROADCAST_MAC, local_mac, 0x0800, &ip_buf[..ip_total], &mut eth_buf)?;

    // Send
    super::send(&eth_buf[..eth_len]).ok()?;

    // Poll for response with timer-based timeout
    let deadline = crate::timer::millis() + DHCP_TIMEOUT_MS;
    while crate::timer::millis() < deadline {
        if let Some(pkt) = super::poll_rx() {
            if let Some(eth) = EthernetFrame::parse(&pkt) {
                if eth.ethertype == ETHERTYPE_IPV4 {
                    if let Some(ip) = Ipv4Packet::parse(eth.payload) {
                        if ip.protocol == 17 {
                            if let Some(udp) = UdpHeader::parse(ip.payload) {
                                if udp.dst_port == DHCP_CLIENT {
                                    if let Some(result) = parse_dhcp_reply(udp.payload, xid, mac) {
                                        return Some(result);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    None
}
