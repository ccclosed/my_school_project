use crate::drivers::pci::{read_config_dword, PciDevice};

/// Intel e1000 driver skeleton — MMIO BAR setup would go here.
#[allow(dead_code)]
pub struct E1000 {
    pub mmio_base: u32,
    pub mac: [u8; 6],
}

#[allow(dead_code)]
impl E1000 {
    pub fn new(pci: PciDevice) -> Self {
        let bar0 = read_config_dword(pci.bus, pci.slot, pci.func, 0x10) & !0xF;
        E1000 {
            mmio_base: bar0,
            mac: probe_mac(pci),
        }
    }

    pub fn reset(&self) {
        let _ = self.mmio_base;
    }

    pub fn send(&self, _frame: &[u8]) -> Result<(), ()> {
        Err(())
    }

    pub fn poll_rx(&self) -> Option<alloc::vec::Vec<u8>> {
        None
    }
}

#[allow(dead_code)]
pub fn default_mac() -> [u8; 6] {
    [0x52, 0x54, 0x00, 0x12, 0x34, 0x56]
}

#[allow(dead_code)]
pub fn probe_mac(pci: PciDevice) -> [u8; 6] {
    let _ = pci;
    default_mac()
}
