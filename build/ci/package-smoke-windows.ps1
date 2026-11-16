# Install the Windows installer as a player would, start the client, run the
# platform smoke on the installed copy, uninstall, and check what is left.
# Writes checks.json, screenshots and logs to -Output; exits 1 if a check failed.
param(
    [Parameter(Mandatory=$true)][string]$Packages,
    [Parameter(Mandatory=$true)][string]$Output,
    # A folder holding an earlier release's installer: installed first, so the new installer runs over it.
    [string]$OldInstallerDir = ''
)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $Output | Out-Null
$Output = (Resolve-Path $Output).Path
$checks = [ordered]@{}
function Check([string]$name, [bool]$ok, $detail) {
    $checks[$name] = [ordered]@{ ok = $ok; detail = "$detail" }
    Write-Host ("{0} {1}: {2}" -f ($(if ($ok) { 'PASS' } else { 'FAIL' }), $name, $detail))
}
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
function Screenshot([string]$path) {
    $bounds = [System.Windows.Forms.SystemInformation]::VirtualScreen
    $bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.CopyFromScreen($bounds.Left, $bounds.Top, 0, 0, $bitmap.Size)
    $bitmap.Save($path, [System.Drawing.Imaging.ImageFormat]::Png)
    $graphics.Dispose(); $bitmap.Dispose()
}

$os = Get-CimInstance Win32_OperatingSystem
Check 'system' $true ("{0} {1} build {2}" -f $os.Caption, $os.Version, $os.BuildNumber)
$installer = Get-ChildItem (Join-Path $Packages '*-win-x64.exe') | Select-Object -First 1
Check 'installer present' ($null -ne $installer) $installer.Name
$signature = Get-AuthenticodeSignature $installer.FullName
Check 'installer signature (informational)' $true $signature.Status

# A per-user install needs no administrator: the installer asks for none.
$dir = Join-Path $env:LOCALAPPDATA 'Programs\Battlefront Eternal'
function UninstallEntries {
    Get-ChildItem 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall' -ErrorAction SilentlyContinue |
        ForEach-Object { Get-ItemProperty $_.PSPath } | Where-Object { $_.DisplayName -like 'Battlefront Eternal*' }
}
if ($OldInstallerDir) {
    $old = Get-ChildItem (Join-Path $OldInstallerDir '*-win-x64.exe') | Select-Object -First 1
    $process = Start-Process -FilePath $old.FullName -ArgumentList '/S', '/currentuser' -Wait -PassThru
    Check 'earlier release installs' ($process.ExitCode -eq 0 -and (Test-Path (Join-Path $dir 'Battlefront Eternal.exe'))) ("{0}: exit {1}" -f $old.Name, $process.ExitCode)
    Get-ChildItem $dir | Select-Object Name, Length | Out-File (Join-Path $Output 'folder-before.txt')
    Check 'earlier uninstall entries (informational)' $true ((UninstallEntries | ForEach-Object { "$($_.DisplayName) $($_.DisplayVersion)" }) -join '; ')
    Get-Process 'Battlefront Eternal' -ErrorAction SilentlyContinue | Stop-Process -Force
}
$started = Get-Date
$process = Start-Process -FilePath $installer.FullName -ArgumentList '/S' -Wait -PassThru
Check 'silent install exit code' ($process.ExitCode -eq 0) ("{0} in {1:n0} s" -f $process.ExitCode, ((Get-Date) - $started).TotalSeconds)
$client = Join-Path $dir 'bfe-client.exe'
Check 'client installed per user' (Test-Path $client) $client
if ($OldInstallerDir) {
    Get-ChildItem $dir | Select-Object Name, Length | Out-File (Join-Path $Output 'folder-after.txt')
    $left = @(Get-ChildItem $dir | ForEach-Object Name | Where-Object { $_ -notin @('bfe-client.exe', 'bfeserver.exe', 'resources', 'Uninstall Battlefront Eternal.exe') })
    Check 'earlier release files removed' ($left.Count -eq 0) ($left -join ', ')
    Check 'no app.asar left' (-not (Test-Path (Join-Path $dir 'resources\app.asar'))) ''
    $entries = @(UninstallEntries)
    Check 'one uninstall entry, the new version' ($entries.Count -eq 1 -and $entries[0].DisplayVersion -eq (Get-Content (Join-Path $dir 'resources\bfe-release.json') -Raw | ConvertFrom-Json).version) (($entries | ForEach-Object { "$($_.DisplayName) $($_.DisplayVersion) $($_.UninstallString)" }) -join '; ')
}
Check 'server installed' (Test-Path (Join-Path $dir 'bfeserver.exe')) $dir
Check 'licence and notices' ((Test-Path (Join-Path $dir 'resources\legal\LICENSE')) -and (Test-Path (Join-Path $dir 'resources\legal\THIRD-PARTY-NOTICES.md'))) 'resources\legal'
$release = Get-Content (Join-Path $dir 'resources\bfe-release.json') -Raw | ConvertFrom-Json
Check 'version' ($release.version -match '^\d+\.\d+\.\d+') ("{0} ({1})" -f $release.version, $release.commit)
$foreign = Get-ChildItem -Recurse $dir | Where-Object { $_.Name -match 'electron|\.asar$|node_modules|^node\.exe$' }
Check 'no Electron or Node.js files' ($foreign.Count -eq 0) (($foreign | ForEach-Object Name) -join ', ')
$size = (Get-ChildItem -Recurse -File $dir | Measure-Object Length -Sum).Sum
Check 'installed size (informational)' $true ("{0:n1} MiB" -f ($size / 1MB))

$shell = New-Object -ComObject WScript.Shell
$startMenu = Join-Path ([Environment]::GetFolderPath('Programs')) 'Battlefront Eternal.lnk'
$desktop = Join-Path ([Environment]::GetFolderPath('Desktop')) 'Battlefront Eternal.lnk'
foreach ($link in @($startMenu, $desktop)) {
    $target = if (Test-Path $link) { $shell.CreateShortcut($link).TargetPath } else { '' }
    Check ("shortcut " + (Split-Path -Leaf (Split-Path -Parent $link))) ($target -ieq $client) "$link -> $target"
}
$key = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Battlefront Eternal'
$entry = Get-ItemProperty $key -ErrorAction SilentlyContinue
Check 'uninstall entry' ($null -ne $entry -and $entry.DisplayVersion -eq $release.version -and $entry.UninstallString -like "*Uninstall Battlefront Eternal.exe*") ("{0} {1}" -f $entry.DisplayName, $entry.DisplayVersion)

# The runner has no GPU: Mesa's llvmpipe beside the client, removed again before uninstalling.
$mesa = Join-Path $env:RUNNER_TEMP 'mesa-dlls'
& (Join-Path $PSScriptRoot 'windows-mesa.ps1') -Destination $mesa
$mesaFiles = Get-ChildItem (Join-Path $mesa '*.dll')
Copy-Item $mesaFiles.FullName $dir

# First start as a new player: through the Start Menu shortcut, no game on this machine.
$env:GALLIUM_DRIVER = 'llvmpipe'
$before = @(Get-Process bfe-client -ErrorAction SilentlyContinue | ForEach-Object Id)
Start-Process -FilePath $startMenu
Start-Sleep -Seconds 30
$running = @(Get-Process bfe-client -ErrorAction SilentlyContinue | Where-Object { $before -notcontains $_.Id })
Check 'first start keeps running' ($running.Count -ge 1) ("{0} process(es)" -f $running.Count)
Screenshot (Join-Path $Output 'first-start.png')
# A player gets the game's window only: no console window (conhost, Windows Terminal) titled after it.
$consoles = @(Get-Process | Where-Object { $_.MainWindowTitle -like '*Battlefront Eternal*' -and $_.ProcessName -ne 'bfe-client' })
Check 'no console window beside the game' ($consoles.Count -eq 0) (($consoles | ForEach-Object { "$($_.ProcessName): $($_.MainWindowTitle)" }) -join '; ')
$running | ForEach-Object { Stop-Process -Id $_.Id -Force }
Start-Sleep -Seconds 2
foreach ($folder in @((Join-Path $env:APPDATA 'Battlefront Eternal'), (Join-Path $env:LOCALAPPDATA 'Battlefront Eternal'))) {
    if (Test-Path $folder) {
        $name = Split-Path -Leaf (Split-Path -Parent $folder)
        Get-ChildItem -Recurse $folder | Select-Object FullName, Length | Out-File (Join-Path $Output "files-$name.txt")
        Get-ChildItem -Recurse -Include *.log, *.json $folder -ErrorAction SilentlyContinue |
            Where-Object { $_.Length -lt 4MB } |
            ForEach-Object { Copy-Item $_.FullName (Join-Path $Output ("$name-" + $_.Name)) }
    }
}

# Startup, main menu and a synthetic room on the installed client.
python tools/platform-smoke-native.py --client-bin $client --output-dir (Join-Path $Output 'platform-smoke')
Check 'platform smoke (startup, menu, room)' ($LASTEXITCODE -eq 0) "exit $LASTEXITCODE"

foreach ($file in $mesaFiles) { Remove-Item (Join-Path $dir $file.Name) -Force }
$uninstaller = Join-Path $dir 'Uninstall Battlefront Eternal.exe'
$process = Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait -PassThru
# The uninstaller runs a copy of itself from the temporary folder and returns at once.
for ($i = 0; $i -lt 60 -and (Test-Path $dir); $i++) { Start-Sleep -Seconds 1 }
$left = if (Test-Path $dir) { (Get-ChildItem -Recurse $dir | ForEach-Object FullName) -join ', ' } else { '' }
Check 'uninstall removes the program folder' (-not (Test-Path $dir)) $left
Check 'uninstall removes shortcuts' (-not (Test-Path $startMenu) -and -not (Test-Path $desktop)) ''
Check 'uninstall removes the uninstall entry' (-not (Test-Path $key)) ''
Check 'player data kept (informational)' $true ("{0}: {1}" -f (Join-Path $env:APPDATA 'Battlefront Eternal'), (Test-Path (Join-Path $env:APPDATA 'Battlefront Eternal')))

$checks | ConvertTo-Json -Depth 4 | Out-File -Encoding utf8 (Join-Path $Output 'checks.json')
$failed = @($checks.Keys | Where-Object { -not $checks[$_].ok })
if ($failed.Count) { Write-Host ("FAILED: " + ($failed -join '; ')); exit 1 }
Write-Host 'all checks passed'
