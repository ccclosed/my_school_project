#[derive(Debug, Clone)]
pub struct Ipv4Packet<'a> {
    pub version: u8,
    pub header_len: u8,
    pub total_len: u16,
    pub protocol: u8,
    pub src: [u8; 4],
    pub dst: [u8; 4],
    pub payload: &'a [u8],
}

fn checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < data.len() {
        sum += u16::from_be_bytes([data[i], data[i + 1]]) as u32;
        i += 2;
    }
    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

impl<'a> Ipv4Packet<'a> {
    pub fn parse(data: &'a [u8]) -> Option<Self> {
        if data.len() < 20 {
            return None;
        }
        let version_ihl = data[0];
        let version = version_ihl >> 4;
        let header_len = (version_ihl & 0x0F) * 4;
        if data.len() < header_len as usize {
            return None;
        }
        let total_len = u16::from_be_bytes([data[2], data[3]]);
        let protocol = data[9];
        let mut src = [0u8; 4];
        let mut dst = [0u8; 4];
        src.copy_from_slice(&data[12..16]);
        dst.copy_from_slice(&data[16..20]);
        Some(Ipv4Packet {
            version,
            header_len,
            total_len,
            protocol,
            src,
            dst,
            payload: &data[header_len as usize..],
        })
    }

    /// Build an IPv4 header into `out`. Returns the header length (20).
    /// `out` must be at least 20 bytes. Checksum is computed automatically.
    pub fn build(
        src: [u8; 4],
        dst: [u8; 4],
        protocol: u8,
        payload_len: u16,
        out: &mut [u8],
    ) -> Option<usize> {
        if out.len() < 20 {
            return None;
        }
        let total_len = 20 + payload_len;
        out[0] = 0x45;              // version=4, ihl=5
        out[1] = 0;                 // DSCP + ECN
        out[2..4].copy_from_slice(&total_len.to_be_bytes());
        out[4..6].copy_from_slice(&[0, 0]); // identification
        out[6..8].copy_from_slice(&[0, 0]); // flags + fragment offset
        out[8] = 64;                // TTL
        out[9] = protocol;
        out[10..12].copy_from_slice(&[0, 0]); // checksum (will fill)
        out[12..16].copy_from_slice(&src);
        out[16..20].copy_from_slice(&dst);

        let cksum = checksum(&out[..20]);
        out[10..12].copy_from_slice(&cksum.to_be_bytes());
        Some(20)
    }
}
