#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct UdpHeader<'a> {
    pub src_port: u16,
    pub dst_port: u16,
    pub length: u16,
    pub payload: &'a [u8],
}

impl<'a> UdpHeader<'a> {
    pub fn parse(data: &'a [u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }
        let src_port = u16::from_be_bytes([data[0], data[1]]);
        let dst_port = u16::from_be_bytes([data[2], data[3]]);
        let length = u16::from_be_bytes([data[4], data[5]]);
        Some(UdpHeader {
            src_port,
            dst_port,
            length,
            payload: &data[8..],
        })
    }

    /// Build a UDP header. `out` must be at least 8 + payload_len bytes.
    /// Checksum is set to 0 (optional for IPv4).
    pub fn build(
        src_port: u16,
        dst_port: u16,
        payload: &[u8],
        out: &mut [u8],
    ) -> Option<usize> {
        let len = 8 + payload.len();
        if out.len() < len {
            return None;
        }
        out[0..2].copy_from_slice(&src_port.to_be_bytes());
        out[2..4].copy_from_slice(&dst_port.to_be_bytes());
        out[4..6].copy_from_slice(&(len as u16).to_be_bytes());
        out[6..8].copy_from_slice(&[0, 0]); // checksum = 0 (optional for IPv4)
        out[8..len].copy_from_slice(payload);
        Some(len)
    }
}
