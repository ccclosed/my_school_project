@echo off
setlocal

if "%1"=="-iso" goto :iso
if "%1"=="--iso" goto :iso

:elf
set ELF=target\x86_64-unknown-none\release\rust-kernel
if not exist "%ELF%" (
    echo Building kernel...
    cargo build --release
    if errorlevel 1 exit /b 1
)
echo Starting QEMU (Multiboot2 -kernel mode)...
qemu-system-x86_64 -machine q35 -kernel "%ELF%" -m 256M -no-reboot -netdev user,id=net0,restrict=off -device rtl8139,netdev=net0 -cpu qemu64 -display gtk -vga std -serial file:kernel.log
goto :end

:iso
set ISO=target\rust-kernel.iso
if not exist "%ISO%" (
    echo ISO not found — run mkiso.ps1 first
    exit /b 1
)
echo Starting QEMU (ISO -cdrom mode)...
qemu-system-x86_64 -machine q35 -cdrom "%ISO%" -m 256M -no-reboot -net nic,model=rtl8139 -net user -cpu qemu64 -display gtk -vga std -serial file:kernel.log

:end

