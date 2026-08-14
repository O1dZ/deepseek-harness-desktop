$ErrorActionPreference = 'Stop'

$root = Split-Path -Parent $PSScriptRoot
$release = Join-Path $root 'release'
$nsis = Join-Path $root 'src-tauri\target\release\bundle\nsis'

$artifacts = @(
    @{
        Source = Join-Path $nsis 'DeepSeek Harness Desktop Lite_0.1.0_x64-setup.exe'
        Destination = Join-Path $release 'DeepSeek-Harness-Desktop-Lite-x64-Setup.exe'
    },
    @{
        Source = Join-Path $nsis 'DeepSeek Harness Desktop Full_0.1.0_x64-setup.exe'
        Destination = Join-Path $release 'DeepSeek-Harness-Desktop-Full-x64-Setup.exe'
    }
)

New-Item -ItemType Directory -Force -Path $release | Out-Null

foreach ($artifact in $artifacts) {
    if (-not (Test-Path -LiteralPath $artifact.Source -PathType Leaf)) {
        throw "Missing installer: $($artifact.Source)"
    }
    Copy-Item -LiteralPath $artifact.Source -Destination $artifact.Destination -Force
}

foreach ($edition in @('Lite', 'Full')) {
    $portableDirectory = Join-Path $release "DeepSeek-Harness-Desktop-$edition-x64-Portable"
    $portableArchive = "$portableDirectory.zip"
    if (-not (Test-Path -LiteralPath $portableDirectory -PathType Container)) {
        throw "Missing staged portable directory: $portableDirectory"
    }
    if (Test-Path -LiteralPath $portableArchive) {
        Remove-Item -LiteralPath $portableArchive -Force
    }
    Compress-Archive -Path (Join-Path $portableDirectory '*') -DestinationPath $portableArchive -CompressionLevel Optimal
}

$checksumPath = Join-Path $release 'SHA256SUMS.txt'
$checksumLines = Get-ChildItem -LiteralPath $release -File |
    Where-Object Name -NotLike 'SHA256SUMS.txt' |
    Sort-Object Name |
    ForEach-Object {
        $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $_.FullName).Hash.ToLowerInvariant()
        "$hash  $($_.Name)"
    }

[System.IO.File]::WriteAllLines($checksumPath, $checksumLines, [System.Text.UTF8Encoding]::new($false))
Write-Host "Release artifacts are ready in $release"
