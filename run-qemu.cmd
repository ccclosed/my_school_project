@echo off
setlocal

if "%1"=="-iso" goto :iso
if "%1"=="--iso" goto :iso

:elf
set ELF=target\i686-unknown-none\release\rust-kernel
if not exist "%ELF%" (
    echo Building kernel...
    cargo build --release
    if errorlevel 1 exit /b 1
)
echo Starting QEMU (ELF -kernel mode)...
qemu-system-i386 -machine pc -kernel "%ELF%" -m 128M -no-reboot -net nic,model=rtl8139 -net user -serial file:kernel.log
goto :end

:iso
set ISO=target\rust-kernel.iso
if not exist "%ISO%" (
    echo ISO not found — run mkiso.ps1 first
    exit /b 1
)
echo Starting QEMU (ISO -cdrom mode)...
qemu-system-i386 -machine pc -cdrom "%ISO%" -m 128M -no-reboot -net nic,model=rtl8139 -net user -serial file:kernel.log

:end
