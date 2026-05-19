use core::arch::global_asm;

global_asm!(
    r#"
.section .multiboot_header, "a"
.align 4
    .long 0x1BADB002
    .long 0
    .long -(0x1BADB002)

.global bootstrap
.global _start
.global load_gdt
.global reload_segments
.global load_idt
.global load_tss
.global isr0
.global isr6
.global isr8
.global isr13
.global isr14
.global irq0
.global irq1
.global irq12
.extern kernel_main
.extern divide_handler
.extern invalid_opcode_handler
.extern double_fault_handler
.extern gpf_handler
.extern page_fault_handler
.extern timer_handler
.extern keyboard_handler
.extern mouse_handler

.section .text
bootstrap:
_start:
    lea esp, [stack_top]
    call kernel_main
.hang:
    hlt
    jmp .hang

load_gdt:
    mov eax, dword ptr [esp + 4]
    lgdt [eax]
    ret

reload_segments:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax
    push 0x08
    lea eax, [reload_cs]
    push eax
    retf
reload_cs:
    ret

load_idt:
    mov eax, dword ptr [esp + 4]
    lidt [eax]
    ret

load_tss:
    mov ax, 0x18        // TSS_SELECTOR
    ltr ax
    ret

isr0:
    pushad
    call divide_handler
    popad
    iretd

// Invalid Opcode (exception 6, no error code)
isr6:
    pushad
    call invalid_opcode_handler
    popad
    iretd

// General Protection Fault (exception 13, has error code on stack)
isr13:
    pushad
    call gpf_handler
    popad
    add esp, 4
    iretd

// Page Fault (exception 14, has error code on stack)
isr14:
    pushad
    call page_fault_handler
    popad
    add esp, 4
    iretd

isr8:
    // Double fault pushes an error code. Save return info from original stack
    // before switching to a dedicated stack (the original may have overflowed).
    mov eax, [esp + 4]   // EIP
    mov ebx, [esp + 8]   // CS
    mov ecx, [esp + 12]  // EFLAGS
    lea esp, [df_stack_top]
    push ecx              // EFLAGS
    push ebx              // CS
    push eax              // EIP
    pushad
    call double_fault_handler
    popad
    iretd

irq0:
    pushad
    push esp             // pass pointer to pushad frame as argument
    call timer_handler   // returns new stack pointer in eax
    add esp, 4           // clean up argument
    mov esp, eax         // switch to next task's stack
    popad
    iretd

irq1:
    pushad
    call keyboard_handler
    popad
    iretd

irq12:
    pushad
    call mouse_handler
    popad
    iretd

.section .bss
.align 16
stack_bottom:
    .space 65536
stack_top:
.align 16
df_stack_bottom:
    .space 2048
df_stack_top:
"#
);
