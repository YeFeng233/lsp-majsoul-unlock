param(
    [string]$Serial = "10.10.1.20:40747",
    [string]$AdbPath = "adb",
    [switch]$RestartGame
)

$ErrorActionPreference = 'Stop'
$source = '/data/user/0/com.yefeng.majmax/files/liqi_config/settings.mod.json'
$targetDirectory = '/data/user/0/com.soulgamechst.majsoul/files/majsoulmax-hook'
$target = "$targetDirectory/settings.mod.json"
$temporary = "$target.tmp"

function Invoke-Adb {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Arguments)
    & $AdbPath -s $Serial @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "adb failed with exit code $LASTEXITCODE"
    }
}

Invoke-Adb -Arguments @('get-state') | Out-Null
Invoke-Adb -Arguments @('shell', 'su', '-c', "test -f $source")
Invoke-Adb -Arguments @('shell', 'su', '-c', "test -d $targetDirectory")

$gameUid = (& $AdbPath -s $Serial shell su -c "stat -c %u $targetDirectory").Trim()
if ($LASTEXITCODE -ne 0 -or $gameUid -notmatch '^\d+$') {
    throw "Cannot determine the game UID: $gameUid"
}

Invoke-Adb -Arguments @('shell', 'su', '-c', "cp $source $temporary")
try {
    Invoke-Adb -Arguments @('shell', 'su', '-c', "chown ${gameUid}:${gameUid} $temporary")
    Invoke-Adb -Arguments @('shell', 'su', '-c', "chmod 600 $temporary")
    Invoke-Adb -Arguments @('shell', 'su', '-c', "mv -f $temporary $target")
} catch {
    & $AdbPath -s $Serial shell su -c "rm -f $temporary" | Out-Null
    throw
}

$digest = (& $AdbPath -s $Serial shell su -c "sha256sum $target").Trim()
if ($LASTEXITCODE -ne 0) {
    throw "Cannot verify the copied settings"
}
Write-Host "Settings copied to the in-process Modder: $digest"

if ($RestartGame) {
    Invoke-Adb -Arguments @('shell', 'am', 'force-stop', 'com.soulgamechst.majsoul')
    Invoke-Adb -Arguments @(
        'shell', 'am', 'start', '-W', '-n',
        'com.soulgamechst.majsoul/com.soulgamechst.mahjongsoulsdk.MainActivity'
    ) | Out-Host
} else {
    Write-Host 'Restart com.soulgamechst.majsoul to load the copied settings.'
}
