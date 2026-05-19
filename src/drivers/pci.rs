use x86::io::{inl, outl};

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

fn pci_config_address(bus: u8, slot: u8, func: u8, offset: u8) -> u32 {
    ((bus as u32) << 16)
        | ((slot as u32) << 11)
        | ((func as u32) << 8)
        | ((offset as u32) & 0xFC)
        | 0x8000_0000
}

pub fn read_config_dword(bus: u8, slot: u8, func: u8, offset: u8) -> u32 {
    unsafe {
        outl(CONFIG_ADDRESS, pci_config_address(bus, slot, func, offset));
        inl(CONFIG_DATA)
    }
}

pub fn write_config_dword(bus: u8, slot: u8, func: u8, offset: u8, value: u32) {
    unsafe {
        outl(CONFIG_ADDRESS, pci_config_address(bus, slot, func, offset));
        outl(CONFIG_DATA, value);
    }
}

/// Enable I/O space (bit 0) and Bus Master (bit 2) on a PCI device.
pub fn enable_bus_mastering(dev: &PciDevice) {
    let mut cmd = read_config_dword(dev.bus, dev.slot, dev.func, 0x04);
    cmd |= 0x05; // bit 0 = I/O enable, bit 2 = bus master enable
    write_config_dword(dev.bus, dev.slot, dev.func, 0x04, cmd);
}

#[derive(Debug, Clone, Copy)]
pub struct PciDevice {
    pub bus: u8,
    pub slot: u8,
    pub func: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class: u8,
    pub subclass: u8,
}

fn read_device(bus: u8, slot: u8, func: u8) -> Option<PciDevice> {
    let dword = read_config_dword(bus, slot, func, 0);
    let vendor = (dword & 0xFFFF) as u16;
    if vendor == 0xFFFF {
        return None;
    }
    let device = ((dword >> 16) & 0xFFFF) as u16;
    let class_rev = read_config_dword(bus, slot, func, 0x08);
    let class = ((class_rev >> 24) & 0xFF) as u8;
    let subclass = ((class_rev >> 16) & 0xFF) as u8;
    Some(PciDevice {
        bus,
        slot,
        func,
        vendor_id: vendor,
        device_id: device,
        class,
        subclass,
    })
}

/// Scan bus 0 and return all found devices.
#[allow(dead_code)]
pub fn scan_bus0() -> alloc::vec::Vec<PciDevice> {
    let mut devices = alloc::vec::Vec::new();
    for slot in 0..32u8 {
        if let Some(dev) = read_device(0, slot, 0) {
            devices.push(dev);
        }
    }
    devices
}

/// Find the first network controller (PCI class 0x02) on bus 0.
/// Read a BAR (Base Address Register) from PCI config space.
pub fn read_bar(dev: &PciDevice, bar: u8) -> u32 {
    read_config_dword(dev.bus, dev.slot, dev.func, 0x10 + bar * 4)
}

pub fn find_network_on_bus0() -> Option<PciDevice> {
    for slot in 0..32u8 {
        if let Some(dev) = read_device(0, slot, 0) {
            if dev.class == 0x02 {
                return Some(dev);
            }
        }
    }
    None
}
