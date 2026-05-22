ENTRY(bootstrap)

SECTIONS
{
    . = 1M;

    .note.Xen : ALIGN(4) {
        KEEP(*(.note.Xen))
    }

    .multiboot2_header : ALIGN(8) {
        KEEP(*(.multiboot2_header))
    }

    .boot : ALIGN(4K) {
        KEEP(*(.boot))
    }

    .text : ALIGN(4K) {
        *(.text .text.*)
    }

    .rodata : ALIGN(4K) {
        *(.rodata .rodata.*)
    }

    .data : ALIGN(4K) {
        *(.data .data.*)
    }

    .bss : ALIGN(4K) {
        PROVIDE(__bss_start = .);
        *(.bss .bss.*)
        *(COMMON)
        PROVIDE(__bss_end = .);
    }

    /DISCARD/ : {
        *(.eh_frame*)
    }
}
