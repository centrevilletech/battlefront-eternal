# Pin both the upstream release and archive bytes before loading any driver DLL.
param([Parameter(Mandatory=$true)][string]$Destination)
$ErrorActionPreference = 'Stop'
# The x64 MinGW driver distribution includes WGL/llvmpipe; the MSVC
# distribution of this release omits OpenGL due to its upstream D3D12 issue.
$uri = 'https://github.com/pal1000/mesa-dist-win/releases/download/26.2.6/mesa3d-26.2.6-release-mingw.7z'
$expected = 'da0f71d9181b672c1918f3c342f11118c687f6059a46b16502845160733cc388'
$stage = Join-Path $env:RUNNER_TEMP 'mesa-llvmpipe'
New-Item -ItemType Directory -Force $stage | Out-Null
$archive = Join-Path $stage 'mesa.7z'
Invoke-WebRequest -Uri $uri -OutFile $archive
if ((Get-FileHash -Algorithm SHA256 $archive).Hash.ToLowerInvariant() -ne $expected) {
    throw 'Mesa archive SHA-256 mismatch'
}
& 'C:\Program Files\7-Zip\7z.exe' x $archive "-o$stage" -y
if ($LASTEXITCODE -ne 0) { throw 'Mesa extraction failed' }
New-Item -ItemType Directory -Force $Destination | Out-Null
# Local DLL deployment leaves the runner's system OpenGL installation untouched.
Copy-Item (Join-Path $stage 'x64\*.dll') $Destination
if (!(Test-Path (Join-Path $Destination 'opengl32.dll'))) { throw 'Mesa OpenGL loader missing' }
if (!(Test-Path (Join-Path $Destination 'libgallium_wgl.dll'))) { throw 'Mesa llvmpipe driver missing' }
