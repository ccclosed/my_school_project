$ErrorActionPreference = "Stop"

$toolchain = "nightly-x86_64-pc-windows-gnu"

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host "ERROR: cargo not found. Install Rust via https://rustup.rs"
    exit 1
}

Write-Host "Updating $toolchain ..."
rustup toolchain install $toolchain | Out-Null
if ($LASTEXITCODE -ne 0) { exit 1 }

Write-Host "Installing components ..."
rustup component add rust-src llvm-tools-preview --toolchain $toolchain | Out-Null
if ($LASTEXITCODE -ne 0) { exit 1 }

$env:KERNEL_RUSTC = rustup which rustc --toolchain $toolchain
if (-not $env:KERNEL_RUSTC) { throw "rustc not found for $toolchain" }

# Build rustc-filter wrapper if needed
$wrapperSrc = "tools\rustc-filter.rs"
$wrapperExe = "tools\rustc-filter.exe"

if (-not (Test-Path $wrapperExe)) {
    Write-Host "Building $wrapperExe ..."
    & $env:KERNEL_RUSTC $wrapperSrc "-o" $wrapperExe
    if ($LASTEXITCODE -ne 0) { throw "rustc-filter build failed" }
}

Write-Host "Using: $env:KERNEL_RUSTC"
Write-Host ""

cargo +$toolchain build --release "-Zbuild-std=core,alloc,compiler_builtins" "-Zbuild-std-features=compiler-builtins-mem" @args
if ($LASTEXITCODE -ne 0) {
    Write-Host "`nBuild failed"
    exit $LASTEXITCODE
}

Write-Host "OK -> target\i686-unknown-none\release\rust-kernel"
