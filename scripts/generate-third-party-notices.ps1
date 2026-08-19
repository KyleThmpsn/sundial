param(
    [string]$Target = "x86_64-pc-windows-msvc",
    [string]$Output = "THIRD_PARTY_NOTICES.md"
)

$ErrorActionPreference = "Stop"

$metadata = cargo metadata --format-version 1 --locked --offline | ConvertFrom-Json
$tree = cargo tree --locked --offline --target $Target -e normal --prefix none --format "{p}"

$wanted = @{}
foreach ($line in $tree) {
    if ($line -match '^(?<name>\S+) v(?<version>\S+)') {
        $wanted["$($Matches.name)|$($Matches.version)"] = $true
    }
}

$packages = $metadata.packages |
    Where-Object {
        $_.name -ne "sundial" -and $wanted.ContainsKey("$($_.name)|$($_.version)")
    } |
    Sort-Object name, version

$parts = [System.Collections.Generic.List[string]]::new()
$parts.Add("# Sundial third-party notices")
$parts.Add("")
$parts.Add("Generated from Cargo.lock for target $Target.")
$parts.Add("Each package and bundled asset remains licensed by its respective authors under the terms shown below.")
$parts.Add("")
$parts.Add("## Feather Icons: Trash 2")
$parts.Add("")
$parts.Add("- License: ``MIT``")
$parts.Add("- Source: https://github.com/feathericons/feather/blob/main/icons/trash-2.svg")
$parts.Add("")
$parts.Add(@'
The MIT License (MIT)

Copyright (c) 2013-2023 Cole Bemis

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
'@.Trim())

foreach ($package in $packages) {
    $parts.Add("")
    $parts.Add("## $($package.name) $($package.version)")
    $parts.Add("")
    $parts.Add(('- SPDX: `{0}`' -f $package.license))
    if ($package.repository) {
        $parts.Add("- Repository: $($package.repository)")
    }

    $packageDirectory = Split-Path -Parent $package.manifest_path
    $licenseFiles = Get-ChildItem -LiteralPath $packageDirectory -Recurse -File |
        Where-Object {
            $_.Name -match '^(LICENSE|LICENCE|COPYING|NOTICE|UNLICENSE|OFL|UFL|COPYRIGHT)(\.|-|$)'
        } |
        Sort-Object FullName

    if (-not $licenseFiles) {
        $parts.Add("")
        $parts.Add("_No separate license text was packaged with this crate; see the SPDX expression and repository above._")
        continue
    }

    foreach ($licenseFile in $licenseFiles) {
        $relative = [System.IO.Path]::GetRelativePath($packageDirectory, $licenseFile.FullName)
        $parts.Add("")
        $parts.Add("### $relative")
        $parts.Add((Get-Content -LiteralPath $licenseFile.FullName -Raw).TrimEnd())
    }
}

$destination = Join-Path (Get-Location) $Output
[System.IO.File]::WriteAllText(
    $destination,
    ($parts -join [Environment]::NewLine) + [Environment]::NewLine,
    [System.Text.UTF8Encoding]::new($false)
)

Write-Output "Wrote $destination for $($packages.Count) packages."
