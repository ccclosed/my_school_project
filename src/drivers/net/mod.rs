pub mod arp;
pub mod dhcp;
pub mod e1000;
pub mod ethernet;
pub mod ipv4;
pub mod rtl8139;
pub mod udp;

use spin::Mutex;

use crate::drivers::pci::{self, PciDevice};
use crate::{info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NicKind {
    None,
    E1000,
    Rtl8139,
}

#[derive(Debug, Clone, Copy)]
pub struct NetStatus {
    pub kind: NicKind,
    pub link_up: bool,
    pub mac: [u8; 6],
    pub pci: Option<PciDevice>,
}

#[derive(Debug, Clone, Copy)]
pub struct NetConfig {
    pub ip: [u8; 4],
    pub subnet: [u8; 4],
    pub gateway: [u8; 4],
    pub dns: [u8; 4],
}

impl NetConfig {
    pub fn display(&self) {
        use crate::info;
        info!(
            "IP: {}.{}.{}.{} / mask: {}.{}.{}.{} / gw: {}.{}.{}.{} / dns: {}.{}.{}.{}",
            self.ip[0], self.ip[1], self.ip[2], self.ip[3],
            self.subnet[0], self.subnet[1], self.subnet[2], self.subnet[3],
            self.gateway[0], self.gateway[1], self.gateway[2], self.gateway[3],
            self.dns[0], self.dns[1], self.dns[2], self.dns[3],
        );
    }
}

static NET_CFG: Mutex<Option<NetConfig>> = Mutex::new(None);

pub fn set_config(cfg: NetConfig) {
    *NET_CFG.lock() = Some(cfg);
}

pub fn get_config() -> Option<NetConfig> {
    *NET_CFG.lock()
}

static NET: Mutex<NetStatus> = Mutex::new(NetStatus {
    kind: NicKind::None,
    link_up: false,
    mac: [0; 6],
    pci: None,
});

pub fn init() {
    let Some(dev) = pci::find_network_on_bus0() else {
        warn!("No network controller found on PCI bus 0");
        return;
    };

    info!(
        "PCI: {:04x}:{:04x} class={:02x} subclass={:02x}",
        dev.vendor_id, dev.device_id, dev.class, dev.subclass
    );

    let status = match (dev.vendor_id, dev.device_id) {
        (0x8086, _) => NetStatus {
            kind: NicKind::E1000,
            link_up: false,
            mac: e1000::default_mac(),
            pci: Some(dev),
        },
        (0x10EC, 0x8139) => {
            pci::enable_bus_mastering(&dev);
            let bar0 = pci::read_bar(&dev, 0);
            let io_base = (bar0 & !0x3) as u16; // mask I/O indicator bits
            let mac = rtl8139::init(io_base);
            NetStatus {
                kind: NicKind::Rtl8139,
                link_up: true,
                mac,
                pci: Some(dev),
            }
        }
        _ => NetStatus {
            kind: NicKind::None,
            link_up: false,
            mac: [0; 6],
            pci: Some(dev),
        },
    };

    *NET.lock() = status;
}

pub fn status() -> NetStatus {
    *NET.lock()
}

/// Send a raw Ethernet frame through the active NIC.
pub fn send(frame: &[u8]) -> Result<(), ()> {
    match NET.lock().kind {
        NicKind::Rtl8139 => rtl8139::send(frame),
        _ => Err(()),
    }
}

/// Return (capr, cbr) from the active NIC.
pub fn rx_ring_pos() -> (u16, u16) {
    match NET.lock().kind {
        NicKind::Rtl8139 => unsafe {
            let io = rtl8139::io_base();
            (x86::io::inw(io + 0x38), x86::io::inw(io + 0x3A))
        },
        _ => (0, 0),
    }
}

/// Enable/disable RTL8139 internal loopback.
pub fn set_loopback(on: bool) {
    rtl8139::set_loopback(on);
}

/// Dump NIC registers for debugging.
pub fn dump_regs() {
    rtl8139::dump_regs();
}

/// Poll the active NIC for a received packet.
pub fn poll_rx() -> Option<alloc::vec::Vec<u8>> {
    match NET.lock().kind {
        NicKind::Rtl8139 => rtl8139::poll_rx(),
        _ => None,
    }
}
