use core::mem::size_of;
use lazy_static::lazy_static;
use spin::Mutex;

#[repr(C, packed)]
pub struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    type_attr: u8,
    offset_mid: u16,
    offset_high: u32,
    reserved: u32,
}

impl IdtEntry {
    fn set_handler_addr(&mut self, handler: usize) {
        let addr = handler as u64;
        self.offset_low = addr as u16;
        self.offset_mid = (addr >> 16) as u16;
        self.offset_high = (addr >> 32) as u32;
        self.selector = crate::gdt::KERNEL_CODE_SELECTOR;
        self.ist = 0;
        self.type_attr = 0x8E;
        self.reserved = 0;
    }
}

#[repr(C, packed)]
struct IdtPointer {
    limit: u16,
    base: u64,
}

const IDT_LEN: usize = 256;

lazy_static! {
    static ref IDT: Mutex<[IdtEntry; IDT_LEN]> = {
        let arr = unsafe { core::mem::zeroed() };
        Mutex::new(arr)
    };
}

extern "C" {
    fn load_idt(ptr: *const IdtPointer);
    fn isr0();
    fn isr6();
    fn isr8();
    fn isr13();
    fn isr14();
    fn irq0();
    fn irq1();
    fn irq12();
}

pub fn init() {
    {
        let mut idt = IDT.lock();
        idt[0].set_handler_addr(isr0 as *const () as usize);
        idt[6].set_handler_addr(isr6 as *const () as usize);
        idt[8].set_handler_addr(isr8 as *const () as usize);
        idt[13].set_handler_addr(isr13 as *const () as usize);
        idt[14].set_handler_addr(isr14 as *const () as usize);
        idt[32].set_handler_addr(irq0 as *const () as usize);
        idt[33].set_handler_addr(irq1 as *const () as usize);
        idt[44].set_handler_addr(irq12 as *const () as usize);
    }

    let idt = IDT.lock();
    let ptr = IdtPointer {
        limit: (size_of::<[IdtEntry; IDT_LEN]>() - 1) as u16,
        base: idt.as_ptr() as u64,
    };
    unsafe {
        load_idt(&ptr);
    }
}
