#[allow(dead_code)]
pub const ETHERTYPE_IPV4: u16 = 0x0800;
pub const ETHERTYPE_ARP: u16 = 0x0806;

#[derive(Debug, Clone)]
pub struct EthernetFrame<'a> {
    pub dst: [u8; 6],
    pub src: [u8; 6],
    pub ethertype: u16,
    pub payload: &'a [u8],
}

impl<'a> EthernetFrame<'a> {
    pub fn parse(data: &'a [u8]) -> Option<Self> {
        if data.len() < 60 {
            return None;
        }
        let mut dst = [0u8; 6];
        let mut src = [0u8; 6];
        dst.copy_from_slice(&data[0..6]);
        src.copy_from_slice(&data[6..12]);
        let ethertype = u16::from_be_bytes([data[12], data[13]]);
        Some(EthernetFrame {
            dst,
            src,
            ethertype,
            payload: &data[14..],
        })
    }

    pub fn build(dst: [u8; 6], src: [u8; 6], ethertype: u16, payload: &[u8], out: &mut [u8]) -> Option<usize> {
        let len = 14 + payload.len();
        if out.len() < len {
            return None;
        }
        out[0..6].copy_from_slice(&dst);
        out[6..12].copy_from_slice(&src);
        out[12..14].copy_from_slice(&ethertype.to_be_bytes());
        out[14..len].copy_from_slice(payload);
        Some(len)
    }
}
