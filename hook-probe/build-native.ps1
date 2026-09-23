param(
    [Parameter(Mandatory = $true)][string]$NdkPath,
    [string]$CargoPath = "cargo",
    [ValidateSet('arm64-v8a', 'armeabi-v7a', 'x86', 'x86_64', 'all')]
    [string[]]$Abis = @('arm64-v8a')
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'native-common.ps1')
$toolBin = Get-AndroidNdkBin $NdkPath
$binarySuffix = if ($env:OS -eq 'Windows_NT') { '.exe' } else { '' }
$compiler = Join-Path $toolBin "clang++$binarySuffix"
if (-not (Test-Path -LiteralPath $compiler)) {
    throw "Android NDK compiler not found: $compiler"
}
& (Join-Path $PSScriptRoot 'build-rust.ps1') -NdkPath $NdkPath -CargoPath $CargoPath -Abis $Abis
if ($LASTEXITCODE -ne 0) { throw "Rust build failed: $LASTEXITCODE" }
foreach ($target in (Get-AndroidNativeTargets $Abis)) {
    $destination = Join-Path $PSScriptRoot "build/native-libs/$($target.abi)"
    $flags = @("--target=$($target.clang)", '-std=c++17', '-shared', '-fPIC', '-O2',
        '-Wall', '-Wextra', '-Werror', '-fvisibility=hidden', '-fno-exceptions', '-fno-rtti',
        '-static-libstdc++', '-Wl,--no-undefined',
        '-Wl,-z,max-page-size=16384', '-Wl,-z,common-page-size=16384')
    if ($target.abi -eq 'armeabi-v7a') { $flags += @('-mthumb', '-mfpu=neon') }
    Write-Host "Building native bridges for $($target.abi)"
    & $compiler @flags (Join-Path $PSScriptRoot 'src/main/cpp/probe.cpp') `
        '-Wl,-rpath,$ORIGIN' -L $destination -lmajsoulmodder -llog -ldl `
        -o (Join-Path $destination 'libmajsoulprobe.so')
    if ($LASTEXITCODE -ne 0) { throw "Native build failed for $($target.abi): $LASTEXITCODE" }
    & $compiler @flags (Join-Path $PSScriptRoot 'src/main/cpp/ai_jni.cpp') `
        '-Wl,-rpath,$ORIGIN' -L $destination -lmajsoulai `
        -o (Join-Path $destination 'libmajsoulai_jni.so')
    if ($LASTEXITCODE -ne 0) { throw "AI JNI build failed for $($target.abi): $LASTEXITCODE" }
}
