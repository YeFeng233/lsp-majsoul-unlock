param(
    [Parameter(Mandatory = $true)][string]$NdkPath,
    [string]$CargoPath = "cargo"
)

$ErrorActionPreference = 'Stop'
$linker = Join-Path $NdkPath 'toolchains\llvm\prebuilt\windows-x86_64\bin\aarch64-linux-android29-clang.cmd'
if (-not (Test-Path -LiteralPath $linker)) {
    throw "Android NDK linker not found: $linker"
}

$manifest = Join-Path $PSScriptRoot 'rust-modder\Cargo.toml'
$destination = Join-Path $PSScriptRoot 'build\native-libs\arm64-v8a'
New-Item -ItemType Directory -Path $destination -Force | Out-Null

$previousLinker = $env:CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER
$previousRustFlags = $env:RUSTFLAGS
$previousCc = $env:CC_aarch64_linux_android
$previousAr = $env:AR_aarch64_linux_android
try {
    $env:CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER = $linker
    $env:RUSTFLAGS = '-C link-arg=-Wl,-z,max-page-size=16384 -C link-arg=-Wl,-z,common-page-size=16384'
    & $CargoPath build --manifest-path $manifest --target aarch64-linux-android --release --locked
    if ($LASTEXITCODE -ne 0) { throw "Rust build failed: $LASTEXITCODE" }
    $env:CC_aarch64_linux_android = $linker
    $env:AR_aarch64_linux_android = Join-Path $NdkPath 'toolchains\llvm\prebuilt\windows-x86_64\bin\llvm-ar.exe'
    & $CargoPath build --manifest-path (Join-Path $PSScriptRoot 'rust-ai\Cargo.toml') --target aarch64-linux-android --release --locked
    if ($LASTEXITCODE -ne 0) { throw "Local AI build failed: $LASTEXITCODE" }
} finally {
    $env:CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER = $previousLinker
    $env:RUSTFLAGS = $previousRustFlags
    $env:CC_aarch64_linux_android = $previousCc
    $env:AR_aarch64_linux_android = $previousAr
}

$source = Join-Path $PSScriptRoot 'rust-modder\target\aarch64-linux-android\release\libmajsoulmodder.so'
if (-not (Test-Path -LiteralPath $source)) {
    throw "Rust output not found: $source"
}
Copy-Item -LiteralPath $source -Destination (Join-Path $destination 'libmajsoulmodder.so') -Force
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'rust-ai\target\aarch64-linux-android\release\libmajsoulai.so') -Destination (Join-Path $destination 'libmajsoulai.so') -Force
