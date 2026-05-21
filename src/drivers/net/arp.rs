#[allow(dead_code)]
pub const ARP_REQUEST: u16 = 1;
#[allow(dead_code)]
pub const ARP_REPLY: u16 = 2;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ArpPacket {
    pub hw_type: u16,
    pub proto_type: u16,
    pub opcode: u16,
    pub sender_mac: [u8; 6],
    pub sender_ip: [u8; 4],
    pub target_mac: [u8; 6],
    pub target_ip: [u8; 4],
}

impl ArpPacket {
    /// Build an ARP request into the `out` buffer. Returns bytes written.
    pub fn build_request(
        sender_mac: [u8; 6],
        sender_ip: [u8; 4],
        target_ip: [u8; 4],
        out: &mut [u8],
    ) -> Option<usize> {
        if out.len() < 28 {
            return None;
        }
        out[..2].copy_from_slice(&1u16.to_be_bytes());     // hw_type = Ethernet
        out[2..4].copy_from_slice(&0x0800u16.to_be_bytes()); // proto_type = IPv4
        out[4] = 6;  // hw_addr_len
        out[5] = 4;  // proto_addr_len
        out[6..8].copy_from_slice(&1u16.to_be_bytes());     // opcode = request
        out[8..14].copy_from_slice(&sender_mac);
        out[14..18].copy_from_slice(&sender_ip);
        out[18..24].copy_from_slice(&[0; 6]);               // target_mac = unknown
        out[24..28].copy_from_slice(&target_ip);
        Some(28)
    }

    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 28 {
            return None;
        }
        let hw_type = u16::from_be_bytes([data[0], data[1]]);
        let proto_type = u16::from_be_bytes([data[2], data[3]]);
        let hlen = data[4];
        let plen = data[5];
        if hw_type != 1 || proto_type != 0x0800 || hlen != 6 || plen != 4 {
            return None;
        }
        let opcode = u16::from_be_bytes([data[6], data[7]]);
        let mut sender_mac = [0u8; 6];
        let mut sender_ip = [0u8; 4];
        let mut target_mac = [0u8; 6];
        let mut target_ip = [0u8; 4];
        sender_mac.copy_from_slice(&data[8..14]);
        sender_ip.copy_from_slice(&data[14..18]);
        target_mac.copy_from_slice(&data[18..24]);
        target_ip.copy_from_slice(&data[24..28]);
        Some(ArpPacket {
            hw_type,
            proto_type,
            opcode,
            sender_mac,
            sender_ip,
            target_mac,
            target_ip,
        })
    }
}
