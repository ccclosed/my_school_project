# Build a bootable ISO with GRUB via WSL Ubuntu.
$ErrorActionPreference = "Stop"
$root = $PSScriptRoot

$elf = "$root\target\x86_64-unknown-none\release\rust-kernel"
if (-not (Test-Path $elf)) {
    Set-Location $root; cargo build --release
    if (-not (Test-Path $elf)) { throw "Build failed" }
}

$isoDir = "$root\target\iso"
$null = New-Item -ItemType Directory -Force -Path "$isoDir\boot\grub"
Copy-Item $elf "$isoDir\boot\rust-kernel" -Force
Copy-Item "$root\boot\grub.cfg" "$isoDir\boot\grub\grub.cfg" -Force

# Convert Windows path to WSL path
# Convert C:\path → /mnt/c/path for WSL
function ConvertTo-WslPath($winPath) {
    $drive = $winPath.Substring(0,1).ToLower()
    $rest = $winPath.Substring(3) -replace '\\', '/'
    "/mnt/$drive/$rest"
}
$wslDir = ConvertTo-WslPath $isoDir
$wslOut = (ConvertTo-WslPath $root) + '/target/rust-kernel.iso'

Write-Host "Creating ISO inside WSL..."
wsl -- bash -c "grub-mkrescue -o '$wslOut' '$wslDir' 2>&1"
if ($LASTEXITCODE -ne 0) { throw "grub-mkrescue failed" }

Write-Host ""
Write-Host "ISO: $root\target\rust-kernel.iso ($((Get-Item "$root\target\rust-kernel.iso").Length / 1KB) KB)"
Write-Host "Run: qemu-system-x86_64 -cdrom '$root\target\rust-kernel.iso' -m 256M -serial file:kernel.log"
