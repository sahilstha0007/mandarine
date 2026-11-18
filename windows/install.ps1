# Mandarine installer for Windows 10/11 (running as the normal user).
#
#   cargo build --release
#   powershell -ExecutionPolicy Bypass -File windows\install.ps1
#
# Installs the binary, adds it to the user PATH, registers Win+Y as a native
# global hotkey at logon, and adds "Convert with Mandarine" to Explorer's file
# context menu plus a Send To shortcut (multi-file).

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$src = Join-Path $root 'target\release\mandarine.exe'
if (-not (Test-Path $src)) { throw "Build first: cargo build --release (missing $src)" }

$installDir = Join-Path $env:LOCALAPPDATA 'Mandarine'
New-Item -ItemType Directory -Force -Path $installDir | Out-Null
$exe = Join-Path $installDir 'mandarine.exe'
Copy-Item -Force $src $exe
Write-Host "==> Installed $exe"

# User PATH (never touches machine PATH).
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($userPath -notlike "*$installDir*") {
    [Environment]::SetEnvironmentVariable('Path', "$userPath;$installDir", 'User')
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    Write-Host '==> Added install dir to user PATH'
}

# GTK4 runtime DLLs live in MSYS2's mingw64\bin. Without it on PATH Windows
# cannot find them and the exe fails with "entry point not found" / 0xc0000135.
$gtkBin = $null
if ($env:MSYSTEM_PREFIX -match 'mingw64') {
    $gtkBin = Join-Path $env:MSYSTEM_PREFIX 'bin'
} else {
    $pkg = Get-Command pkg-config.exe -ErrorAction SilentlyContinue
    if ($pkg -and $pkg.Source -match '^(.*\\mingw64)\\bin\\') { $gtkBin = Join-Path $Matches[1] 'bin' }
}
if ($gtkBin -and (Test-Path $gtkBin)) {
    if ($userPath -notlike "*$gtkBin*") {
        [Environment]::SetEnvironmentVariable('Path', "$userPath;$gtkBin", 'User')
        Write-Host "==> Added $gtkBin to user PATH (GTK4 runtime DLLs)"
    }
} else {
    Write-Warning 'MSYS2 mingw64 GTK4 runtime not found. Install with: pacman -S mingw-w64-x86_64-gtk4 — then keep its bin\ on PATH.'
}

# Fail here, not on the first Win+Y press, if the binary cannot start.
try {
    $ver = & $exe --version 2>&1
    Write-Host "==> Verified binary: $ver"
} catch {
    Write-Warning "Binary did not start: $_. Check that the MSYS2 mingw64\\bin DLLs are on PATH."
}

# Native hotkey: Win+Y starts the resident daemon. RegisterHotKey lives in the
# app itself; this just starts it at logon.
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
New-ItemProperty -Force -Path $runKey -Name 'Mandarine' `
    -Value ('"' + $exe + '" daemon') | Out-Null
Write-Host '==> Registered Win+Y hotkey at logon'

# Explorer context menu for files and folders.
foreach ($scope in @('*', 'Directory')) {
    $verb = "HKCU:\Software\Classes\$scope\shell\Mandarine"
    New-Item -Force -Path $verb | Out-Null
    Set-Item -Force -Path $verb -Value 'Convert with Mandarine'
    Set-ItemProperty -Force -Path $verb -Name 'Icon' -Value $exe
    $cmd = Join-Path $verb 'command'
    New-Item -Force -Path $cmd | Out-Null
    Set-Item -Force -Path $cmd -Value ('"' + $exe + '" wheel --files "%1"')
}
Write-Host '==> Added Explorer context menu'

# Send To supports multiple selected files (unlike the classic context verb).
$sendTo = Join-Path $env:APPDATA 'Microsoft\Windows\SendTo'
$wsh = New-Object -ComObject WScript.Shell
$link = $wsh.CreateShortcut((Join-Path $sendTo 'Mandarine.lnk'))
$link.TargetPath = $exe
$link.Arguments = 'wheel --files'
$link.Description = 'Convert selected files with Mandarine'
$link.Save()
Write-Host '==> Added Send To shortcut'

Write-Host @'

Next steps
  1. Sign out and back in (PATH + Run key + Explorer shell changes take effect).
  2. Press Win+Y for the wheel, or right-click files -> Convert with Mandarine.
  3. GTK4 must be installed (MSYS2: pacman -S mingw-w64-x86_64-gtk4) and
     mandarine.exe on PATH alongside it.
  4. Optional engines (ffmpeg, magick, etc.) hide their targets when absent.
'@
