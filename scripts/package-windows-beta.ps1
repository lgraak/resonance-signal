[CmdletBinding()]
param(
    [string]$OutputRoot
)

$ErrorActionPreference = 'Stop'

function Get-PeSubsystem {
    param([Parameter(Mandatory)][string]$Path)

    $stream = [System.IO.File]::OpenRead($Path)
    $reader = [System.IO.BinaryReader]::new($stream)
    try {
        $stream.Position = 0x3c
        $peOffset = $reader.ReadInt32()
        $stream.Position = $peOffset
        if ($reader.ReadUInt32() -ne 0x00004550) {
            throw "PE signature is missing from '$Path'."
        }
        $stream.Position = $peOffset + 4 + 20 + 68
        return $reader.ReadUInt16()
    }
    finally {
        $reader.Dispose()
        $stream.Dispose()
    }
}

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
    $metadataJson = & cargo metadata --locked --format-version 1 --no-deps
    if ($LASTEXITCODE -ne 0) {
        throw "Cargo metadata failed with exit code $LASTEXITCODE."
    }
    $metadata = $metadataJson -join [System.Environment]::NewLine | ConvertFrom-Json
    $agentPackages = @($metadata.packages | Where-Object { $_.name -eq 'resonance-agent' })
    if ($agentPackages.Count -ne 1) {
        throw "Expected exactly one resonance-agent package in Cargo metadata; found $($agentPackages.Count)."
    }
    $packageVersion = [string]$agentPackages[0].version
    if ([string]::IsNullOrWhiteSpace($packageVersion)) {
        throw 'Cargo metadata did not provide a resonance-agent package version.'
    }

    & cargo build --release --locked -p resonance-agent
    if ($LASTEXITCODE -ne 0) {
        throw "Release build failed with exit code $LASTEXITCODE."
    }

    $releaseExecutable = Join-Path $repositoryRoot 'target\release\resonance-agent.exe'
    $releaseCliExecutable = Join-Path $repositoryRoot 'target\release\resonance-agent-cli.exe'
    if ((Get-PeSubsystem -Path $releaseExecutable) -ne 2) {
        throw 'resonance-agent.exe must use the Windows GUI subsystem for console-free tray launch.'
    }
    if ((Get-PeSubsystem -Path $releaseCliExecutable) -ne 3) {
        throw 'resonance-agent-cli.exe must use the Windows console subsystem for synchronous CLI behavior.'
    }
    $actualVersionOutput = (& $releaseCliExecutable --version) -join [System.Environment]::NewLine
    if ($LASTEXITCODE -ne 0) {
        throw "Built executable version check failed with exit code $LASTEXITCODE."
    }
    $expectedVersionOutput = "resonance-agent $packageVersion"
    if ($actualVersionOutput.Trim() -cne $expectedVersionOutput) {
        throw "Built executable version '$($actualVersionOutput.Trim())' does not match Cargo metadata '$expectedVersionOutput'."
    }

    $packageName = "resonance-signal-$packageVersion-windows-x64"
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
    Copy-Item -LiteralPath $releaseExecutable -Destination $packageDirectory
    Copy-Item -LiteralPath $releaseCliExecutable -Destination $packageDirectory
    Copy-Item -LiteralPath (Join-Path $repositoryRoot 'LICENSE') -Destination (Join-Path $packageDirectory 'LICENSE.txt')
    Copy-Item -LiteralPath (Join-Path $repositoryRoot 'packaging\windows-beta\README.txt') -Destination (Join-Path $packageDirectory 'README.txt')
    Compress-Archive -Path (Join-Path $packageDirectory '*') -DestinationPath $archivePath -CompressionLevel Optimal

    Write-Output $archivePath
}
finally {
    Pop-Location
}
