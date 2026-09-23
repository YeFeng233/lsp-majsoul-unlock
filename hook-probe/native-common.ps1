function Get-AndroidNativeTargets {
    param([string[]]$Abis)
    $targets = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'native-targets.json') -Raw | ConvertFrom-Json
    if ($Abis -contains 'all') { return $targets }
    foreach ($abi in ($Abis | Select-Object -Unique)) {
        $target = $targets | Where-Object abi -EQ $abi
        if (-not $target) { throw "Unsupported Android ABI: $abi" }
        $target
    }
}

function Get-AndroidNdkBin {
    param([string]$NdkPath)
    $hostTag = if ($env:OS -eq 'Windows_NT') { 'windows-x86_64' }
        elseif ($IsMacOS) { 'darwin-x86_64' } else { 'linux-x86_64' }
    Join-Path $NdkPath "toolchains/llvm/prebuilt/$hostTag/bin"
}
