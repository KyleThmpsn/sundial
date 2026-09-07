[CmdletBinding()]
param(
    [string]$Version,
    [string]$TargetDirectory,
    [string]$DistDirectory,
    [string]$SundialBinary
)

$ErrorActionPreference = "Stop"

$repoDirectory = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))

function Resolve-RepositoryPath {
    param([Parameter(Mandatory)][string]$Path)

    if ([System.IO.Path]::IsPathRooted($Path)) {
        return [System.IO.Path]::GetFullPath($Path)
    }
    return [System.IO.Path]::GetFullPath((Join-Path $repoDirectory $Path))
}

if ([string]::IsNullOrWhiteSpace($Version)) {
    $manifest = Get-Content -LiteralPath (Join-Path $repoDirectory "Cargo.toml") -Raw
    $versionMatch = [regex]::Match($manifest, '(?m)^version = "([^"]+)"')
    if (-not $versionMatch.Success) {
        throw "Could not read the Sundial version from Cargo.toml"
    }
    $Version = $versionMatch.Groups[1].Value
}
$Version = $Version -replace '^v', ''
if ($Version -notmatch '^[0-9A-Za-z][0-9A-Za-z.+-]*$') {
    throw "Release version contains unsafe filename characters: $Version"
}

if ([string]::IsNullOrWhiteSpace($TargetDirectory)) {
    $TargetDirectory = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { "target" }
}
$TargetDirectory = Resolve-RepositoryPath $TargetDirectory

if ([string]::IsNullOrWhiteSpace($DistDirectory)) {
    $DistDirectory = if ($env:SUNDIAL_DIST_DIR) { $env:SUNDIAL_DIST_DIR } else { "dist" }
}
$DistDirectory = Resolve-RepositoryPath $DistDirectory

if ([string]::IsNullOrWhiteSpace($SundialBinary)) {
    $SundialBinary = if ($env:SUNDIAL_BINARY) {
        $env:SUNDIAL_BINARY
    } else {
        Join-Path $TargetDirectory "release/sundial.exe"
    }
}
$SundialBinary = Resolve-RepositoryPath $SundialBinary

$releaseFiles = [ordered]@{
    "sundial.exe" = $SundialBinary
    "LICENSE" = Join-Path $repoDirectory "LICENSE"
    "README.md" = Join-Path $repoDirectory "README.md"
    "assets/sundial-alt.png" = Join-Path $repoDirectory "assets/sundial-alt.png"
    "crates/parhelion/README.md" = Join-Path $repoDirectory "crates/parhelion/README.md"
    "THIRD_PARTY_NOTICES.txt" = Join-Path $repoDirectory "packaging/THIRD_PARTY_NOTICES.txt"
}
foreach ($source in $releaseFiles.Values) {
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
        throw "Missing required release file: $source"
    }
}

$bundleName = "Sundial-v$Version-windows-x86_64"
$temporaryBase = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$stageRoot = Join-Path $temporaryBase ("sundial-release-" + [guid]::NewGuid().ToString("N"))
$bundleDirectory = Join-Path $stageRoot $bundleName

[System.IO.Directory]::CreateDirectory($bundleDirectory) | Out-Null
[System.IO.Directory]::CreateDirectory($DistDirectory) | Out-Null

try {
    foreach ($entry in $releaseFiles.GetEnumerator()) {
        $destination = Join-Path $bundleDirectory $entry.Key
        [System.IO.Directory]::CreateDirectory([System.IO.Path]::GetDirectoryName($destination)) | Out-Null
        Copy-Item -LiteralPath $entry.Value -Destination $destination
    }

    $archive = Join-Path $DistDirectory "$bundleName.zip"
    Compress-Archive -LiteralPath $bundleDirectory -DestinationPath $archive -CompressionLevel Optimal -Force

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [System.IO.Compression.ZipFile]::OpenRead($archive)
    try {
        $actual = @(
            $zip.Entries |
                ForEach-Object { $_.FullName.Replace('\', '/') } |
                Where-Object { -not $_.EndsWith('/') } |
                Sort-Object -Unique
        )
        $expected = @(
            $releaseFiles.Keys |
                ForEach-Object { "$bundleName/$_" } |
                Sort-Object -Unique
        )
        if (Compare-Object -ReferenceObject $expected -DifferenceObject $actual) {
            throw "Packaged archive does not match the reviewed release allowlist"
        }
    } finally {
        $zip.Dispose()
    }

    $hash = Get-FileHash -LiteralPath $archive -Algorithm SHA256
    $hashLine = "$($hash.Hash.ToLowerInvariant())  $([System.IO.Path]::GetFileName($archive))"
    Set-Content -LiteralPath "$archive.sha256" -Value $hashLine -Encoding ascii

    Write-Output "Created $archive"
} finally {
    $resolvedStageRoot = [System.IO.Path]::GetFullPath($stageRoot)
    if (-not $resolvedStageRoot.StartsWith($temporaryBase, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Refusing to remove unexpected staging path: $resolvedStageRoot"
    }
    if ([System.IO.Directory]::Exists($resolvedStageRoot)) {
        [System.IO.Directory]::Delete($resolvedStageRoot, $true)
    }
}
