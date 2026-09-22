param(
    [Parameter(Mandatory = $true)][string]$NdkPath,
    [string]$CargoPath = "cargo"
)

$ErrorActionPreference = 'Stop'
$compiler = Join-Path $NdkPath 'toolchains\llvm\prebuilt\windows-x86_64\bin\clang++.exe'
if (-not (Test-Path -LiteralPath $compiler)) {
    throw "Android NDK compiler not found: $compiler"
}
$destination = Join-Path $PSScriptRoot 'build\native-libs\arm64-v8a'
New-Item -ItemType Directory -Path $destination -Force | Out-Null
& (Join-Path $PSScriptRoot 'build-rust.ps1') -NdkPath $NdkPath -CargoPath $CargoPath
if ($LASTEXITCODE -ne 0) { throw "Rust build failed: $LASTEXITCODE" }
& $compiler --target=aarch64-linux-android29 -std=c++17 -shared -fPIC -O2 `
    -Wall -Wextra -Werror -fvisibility=hidden -fno-exceptions -fno-rtti -static-libstdc++ `
    '-Wl,-z,max-page-size=16384' '-Wl,-z,common-page-size=16384' `
    (Join-Path $PSScriptRoot 'src\main\cpp\probe.cpp') `
    '-Wl,-rpath,$ORIGIN' -L $destination -lmajsoulmodder -llog -ldl `
    -o (Join-Path $destination 'libmajsoulprobe.so')
if ($LASTEXITCODE -ne 0) { throw "Native build failed: $LASTEXITCODE" }
