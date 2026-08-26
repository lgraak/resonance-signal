[CmdletBinding()]
param(
    [string]$OutputRoot
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if ([string]::IsNullOrWhiteSpace($OutputRoot)) {
    $OutputRoot = Join-Path $repositoryRoot 'dist'
}
$outputRootPath = [System.IO.Path]::GetFullPath($OutputRoot)
$repositoryPrefix = $repositoryRoot.TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
if (-not $outputRootPath.StartsWith($repositoryPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "OutputRoot must remain inside the repository: $repositoryRoot"
}

$rustHost = (& rustc -vV | Select-String '^host:' | ForEach-Object { $_.Line.Split(':', 2)[1].Trim() })
if ($LASTEXITCODE -ne 0 -or $rustHost -ne 'x86_64-pc-windows-msvc') {
    throw "Windows beta packaging requires the x86_64-pc-windows-msvc Rust host; found '$rustHost'."
}

Push-Location $repositoryRoot
try {
    & cargo build --release --locked -p resonance-agent
    if ($LASTEXITCODE -ne 0) {
        throw "Release build failed with exit code $LASTEXITCODE."
    }

    $packageName = 'resonance-signal-windows-x64'
    $packageDirectory = Join-Path $outputRootPath $packageName
    $archivePath = Join-Path $outputRootPath "$packageName.zip"
    foreach ($target in @($packageDirectory, $archivePath)) {
        $resolvedTarget = [System.IO.Path]::GetFullPath($target)
        $outputPrefix = $outputRootPath.TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
        if (-not $resolvedTarget.StartsWith($outputPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
            throw "Refusing to replace an unexpected package target: $resolvedTarget"
        }
        if (Test-Path -LiteralPath $resolvedTarget) {
            Remove-Item -LiteralPath $resolvedTarget -Recurse -Force
        }
    }

    New-Item -ItemType Directory -Path $packageDirectory | Out-Null
    Copy-Item -LiteralPath (Join-Path $repositoryRoot 'target\release\resonance-agent.exe') -Destination $packageDirectory
    Copy-Item -LiteralPath (Join-Path $repositoryRoot 'LICENSE') -Destination (Join-Path $packageDirectory 'LICENSE.txt')
    Copy-Item -LiteralPath (Join-Path $repositoryRoot 'packaging\windows-beta\README.txt') -Destination (Join-Path $packageDirectory 'README.txt')
    Compress-Archive -Path (Join-Path $packageDirectory '*') -DestinationPath $archivePath -CompressionLevel Optimal

    Write-Output $archivePath
}
finally {
    Pop-Location
}
