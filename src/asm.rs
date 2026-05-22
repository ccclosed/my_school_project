use core::arch::global_asm;

global_asm!(
    r#"
.section .note.Xen, "a"
.align 4
    .long 4                             // namesz
    .long 12                            // descsz
    .long 18                            // type = XEN_ELFNOTE_PHYS32_ENTRY
    .asciz "Xen"
    .align 4
    .long bootstrap                     // entry point
    .long 0                             // offset
    .long 0                             // flags

.section .multiboot2_header, "a"
.align 8
multiboot2_header_start:
    .long 0xE85250D6                    // magic
    .long 0                             // architecture (0 = i386)
    .long multiboot2_header_end - multiboot2_header_start
    .long -(0xE85250D6 + 0 + (multiboot2_header_end - multiboot2_header_start))
    
    // End tag
    .word 0    // type
    .word 0    // flags
    .long 8    // size
multiboot2_header_end:

.section .boot, "ax"
.code32
.global bootstrap
.global _start

bootstrap:
_start:
    cli
    mov esp, offset boot_stack_top
    
    // Save Multiboot2 magic and info pointer
    mov dword ptr [mb2_magic], eax
    mov dword ptr [mb2_info], ebx
    
    // Check for CPUID support
    pushfd
    pop eax
    mov ecx, eax
    xor eax, 0x200000
    push eax
    popfd
    pushfd
    pop eax
    push ecx
    popfd
    xor eax, ecx
    jz no_long_mode
    
    // Check for long mode support
    mov eax, 0x80000000
    cpuid
    cmp eax, 0x80000001
    jb no_long_mode
    
    mov eax, 0x80000001
    cpuid
    test edx, (1 << 29)
    jz no_long_mode
    
    // Setup page tables for long mode
    // PML4[0] -> PDPT
    mov eax, offset pdpt
    or eax, 0x3
    mov dword ptr [pml4], eax
    
    // PDPT[0] -> PD
    mov eax, offset pd
    or eax, 0x3
    mov dword ptr [pdpt], eax
    
    // Identity map first 1GB using 2MB pages
    mov ecx, 0
    mov eax, 0x83  // present, writable, huge page
.fill_pd:
    mov dword ptr [pd + ecx * 8], eax
    mov dword ptr [pd + ecx * 8 + 4], 0
    add eax, 0x200000
    inc ecx
    cmp ecx, 512
    jl .fill_pd
    
    // Load PML4 into CR3
    mov eax, offset pml4
    mov cr3, eax
    
    // Enable PAE
    mov eax, cr4
    or eax, (1 << 5)
    mov cr4, eax
    
    // Enable long mode (set EFER.LME)
    mov ecx, 0xC0000080
    rdmsr
    or eax, (1 << 8)
    wrmsr
    
    // Enable paging and protected mode
    mov eax, cr0
    or eax, (1 << 31) | (1 << 0)
    mov cr0, eax
    
    // Load 64-bit GDT
    lgdt [gdt64_pointer]
    
    // Far jump to 64-bit code
    push 0x08
    lea eax, [long_mode_start]
    push eax
    retf

no_long_mode:
    hlt
    jmp no_long_mode

.code64
long_mode_start:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax
    
    // Enable SSE (required for x86_64 ABI)
    mov rax, cr0
    and rax, 0xFFFFFFFFFFFFFFFB      // Clear CR0.EM (bit 2)
    or rax, 0x0002                   // Set CR0.MP (bit 1)
    mov cr0, rax
    
    mov rax, cr4
    or rax, 0x0600                   // Set CR4.OSFXSR (bit 9) and CR4.OSXMMEXCPT (bit 10)
    mov cr4, rax
    
    lea rsp, [stack_top]
    call kernel_main
.hang:
    hlt
    jmp .hang

.global load_gdt
load_gdt:
    lgdt [rdi]
    ret

.global reload_segments
reload_segments:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax
    push 0x08
    lea rax, [reload_cs]
    push rax
    retfq
reload_cs:
    ret

.global load_idt
load_idt:
    lidt [rdi]
    ret

.global load_tss
load_tss:
    mov ax, 0x28
    ltr ax
    ret

.global isr0
isr0:
    push 0
    push 0
    push r15
    push r14
    push r13
    push r12
    push r11
    push r10
    push r9
    push r8
    push rdi
    push rsi
    push rbp
    push rdx
    push rcx
    push rbx
    push rax
    call divide_handler
    pop rax
    pop rbx
    pop rcx
    pop rdx
    pop rbp
    pop rsi
    pop rdi
    pop r8
    pop r9
    pop r10
    pop r11
    pop r12
    pop r13
    pop r14
    pop r15
    add rsp, 16
    iretq

.global isr6
isr6:
    push 0
    push 6
    push r15
    push r14
    push r13
    push r12
    push r11
    push r10
    push r9
    push r8
    push rdi
    push rsi
    push rbp
    push rdx
    push rcx
    push rbx
    push rax
    call invalid_opcode_handler
    pop rax
    pop rbx
    pop rcx
    pop rdx
    pop rbp
    pop rsi
    pop rdi
    pop r8
    pop r9
    pop r10
    pop r11
    pop r12
    pop r13
    pop r14
    pop r15
    add rsp, 16
    iretq

.global isr8
isr8:
    // Double fault - switch to dedicated stack IMMEDIATELY
    // Error code already on stack from CPU
    lea rsp, [df_stack_top]
    push 8
    push r15
    push r14
    push r13
    push r12
    push r11
    push r10
    push r9
    push r8
    push rdi
    push rsi
    push rbp
    push rdx
    push rcx
    push rbx
    push rax
    call double_fault_handler
    // Double fault handler should never return
.df_hang:
    cli
    hlt
    jmp .df_hang

.global isr13
isr13:
    // GPF has error code on stack
    push r15
    push r14
    push r13
    push r12
    push r11
    push r10
    push r9
    push r8
    push rdi
    push rsi
    push rbp
    push rdx
    push rcx
    push rbx
    push rax
    mov rdi, [rsp + 15*8]  // Get error code from stack
    call gpf_handler
    pop rax
    pop rbx
    pop rcx
    pop rdx
    pop rbp
    pop rsi
    pop rdi
    pop r8
    pop r9
    pop r10
    pop r11
    pop r12
    pop r13
    pop r14
    pop r15
    add rsp, 8  // Pop error code
    iretq

.global isr14
isr14:
    // Page fault has error code on stack
    push r15
    push r14
    push r13
    push r12
    push r11
    push r10
    push r9
    push r8
    push rdi
    push rsi
    push rbp
    push rdx
    push rcx
    push rbx
    push rax
    mov rdi, [rsp + 15*8]  // Get error code from stack
    call page_fault_handler
    pop rax
    pop rbx
    pop rcx
    pop rdx
    pop rbp
    pop rsi
    pop rdi
    pop r8
    pop r9
    pop r10
    pop r11
    pop r12
    pop r13
    pop r14
    pop r15
    add rsp, 8  // Pop error code
    iretq

.global irq0
irq0:
    push r15
    push r14
    push r13
    push r12
    push r11
    push r10
    push r9
    push r8
    push rdi
    push rsi
    push rbp
    push rdx
    push rcx
    push rbx
    push rax
    mov rdi, rsp
    call timer_handler
    mov rsp, rax
    pop rax
    pop rbx
    pop rcx
    pop rdx
    pop rbp
    pop rsi
    pop rdi
    pop r8
    pop r9
    pop r10
    pop r11
    pop r12
    pop r13
    pop r14
    pop r15
    iretq

.global irq1
irq1:
    push r15
    push r14
    push r13
    push r12
    push r11
    push r10
    push r9
    push r8
    push rdi
    push rsi
    push rbp
    push rdx
    push rcx
    push rbx
    push rax
    call keyboard_handler
    pop rax
    pop rbx
    pop rcx
    pop rdx
    pop rbp
    pop rsi
    pop rdi
    pop r8
    pop r9
    pop r10
    pop r11
    pop r12
    pop r13
    pop r14
    pop r15
    iretq

.global irq12
irq12:
    push r15
    push r14
    push r13
    push r12
    push r11
    push r10
    push r9
    push r8
    push rdi
    push rsi
    push rbp
    push rdx
    push rcx
    push rbx
    push rax
    call mouse_handler
    pop rax
    pop rbx
    pop rcx
    pop rdx
    pop rbp
    pop rsi
    pop rdi
    pop r8
    pop r9
    pop r10
    pop r11
    pop r12
    pop r13
    pop r14
    pop r15
    iretq

.section .data
.align 16
gdt64:
    .quad 0                             // Null descriptor
    .quad 0x00AF9A000000FFFF            // Code segment (64-bit)
    .quad 0x00CF92000000FFFF            // Data segment
gdt64_pointer:
    .word gdt64_pointer - gdt64 - 1
    .quad gdt64

.section .bss
.align 8
mb2_magic:
    .space 4
mb2_info:
    .space 4

.align 4096
pml4:
    .space 4096
pdpt:
    .space 4096
pd:
    .space 4096

.align 16
boot_stack_bottom:
    .space 16384
boot_stack_top:

.align 16
stack_bottom:
    .space 262144
stack_top:

.align 16
df_stack_bottom:
    .space 8192
df_stack_top:
"#
);
