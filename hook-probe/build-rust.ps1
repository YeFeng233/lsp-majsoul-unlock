param(
    [Parameter(Mandatory = $true)][string]$NdkPath,
    [string]$CargoPath = "cargo",
    [ValidateSet('arm64-v8a', 'armeabi-v7a', 'x86', 'x86_64', 'all')]
    [string[]]$Abis = @('arm64-v8a')
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'native-common.ps1')
$toolBin = Get-AndroidNdkBin $NdkPath
$scriptSuffix = if ($env:OS -eq 'Windows_NT') { '.cmd' } else { '' }
$binarySuffix = if ($env:OS -eq 'Windows_NT') { '.exe' } else { '' }
$targetDirectory = Join-Path $PSScriptRoot 'build/rust'

foreach ($target in (Get-AndroidNativeTargets $Abis)) {
    $linker = Join-Path $toolBin "$($target.clang)-clang$scriptSuffix"
    if (-not (Test-Path -LiteralPath $linker)) { throw "Android NDK linker not found: $linker" }
    $targetKey = $target.rust.Replace('-', '_')
    $settings = @{
        "CARGO_TARGET_$($targetKey.ToUpperInvariant())_LINKER" = $linker
        "CC_$targetKey" = $linker
        "CXX_$targetKey" = Join-Path $toolBin "$($target.clang)-clang++$scriptSuffix"
        "AR_$targetKey" = Join-Path $toolBin "llvm-ar$binarySuffix"
        'RUSTFLAGS' = "$env:RUSTFLAGS -C link-arg=-Wl,-z,max-page-size=16384 -C link-arg=-Wl,-z,common-page-size=16384".Trim()
    }
    $previous = @{}
    try {
        foreach ($key in $settings.Keys) {
            $previous[$key] = [Environment]::GetEnvironmentVariable($key, 'Process')
            [Environment]::SetEnvironmentVariable($key, $settings[$key], 'Process')
        }
        foreach ($crate in @('rust-modder', 'rust-ai')) {
            Write-Host "Building $crate for $($target.abi)"
            & $CargoPath build --manifest-path (Join-Path $PSScriptRoot "$crate/Cargo.toml") `
                --target $target.rust --target-dir $targetDirectory --release --locked
            if ($LASTEXITCODE -ne 0) { throw "$crate build failed for $($target.abi): $LASTEXITCODE" }
        }
    } finally {
        foreach ($key in $previous.Keys) {
            [Environment]::SetEnvironmentVariable($key, $previous[$key], 'Process')
        }
    }
    $destination = Join-Path $PSScriptRoot "build/native-libs/$($target.abi)"
    New-Item -ItemType Directory -Path $destination -Force | Out-Null
    foreach ($library in @('libmajsoulmodder.so', 'libmajsoulai.so')) {
        Copy-Item -LiteralPath (Join-Path $targetDirectory "$($target.rust)/release/$library") `
            -Destination (Join-Path $destination $library) -Force
    }
}
