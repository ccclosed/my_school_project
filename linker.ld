ENTRY(bootstrap)

SECTIONS
{
    . = 1M;

    .multiboot : ALIGN(4) {
        KEEP(*(.multiboot_header))
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

    PROVIDE(__stack_bottom = ADDR(.bss) + SIZEOF(.bss));
    PROVIDE(__stack_top = __stack_bottom + 64K);

    /DISCARD/ : {
        *(.eh_frame*)
    }
}
