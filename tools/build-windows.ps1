$ErrorActionPreference = 'Stop'
$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$tauriCli = Join-Path $repoRoot 'node_modules\.bin\tauri.cmd'
if (-not (Test-Path -LiteralPath $tauriCli -PathType Leaf)) {
    throw 'Run npm ci before building DumbManager Rust.'
}
Push-Location -LiteralPath $repoRoot
try {
    & $tauriCli build --ci --no-bundle
    if ($LASTEXITCODE -ne 0) { throw 'Desktop build failed.' }
    & cargo build --manifest-path src-tauri/Cargo.toml --release --locked --bin dumbmanager-mcp
    if ($LASTEXITCODE -ne 0) { throw 'MCP console build failed.' }
    # Add the already-built console only at bundle time. This avoids making
    # cargo's first compile depend on an executable it has not produced yet.
    & $tauriCli bundle --ci --bundles nsis --config src-tauri/tauri.bundle.conf.json
    if ($LASTEXITCODE -ne 0) { throw 'Windows installer packaging failed.' }
} finally {
    Pop-Location
}
