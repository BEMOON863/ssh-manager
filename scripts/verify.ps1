Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Проверки из AGENTS.md: fmt -> check -> clippy. Останавливаемся на первой ошибке.
$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $repoRoot
try {
    $steps = @(
        @{ Name = 'cargo fmt --check'; Arguments = @('fmt', '--check') },
        @{ Name = 'cargo check --all-targets'; Arguments = @('check', '--all-targets') },
        @{ Name = 'cargo clippy --all-targets -- -D warnings'; Arguments = @('clippy', '--all-targets', '--', '-D', 'warnings') }
    )

    foreach ($step in $steps) {
        Write-Host "==> $($step.Name)"
        & cargo @($step.Arguments)
        if ($LASTEXITCODE -ne 0) {
            Write-Host "FAIL: $($step.Name) (exit code $LASTEXITCODE)"
            exit $LASTEXITCODE
        }
        Write-Host "PASS: $($step.Name)"
    }

    Write-Host 'ALL CHECKS PASSED'
    exit 0
}
finally {
    Pop-Location
}
