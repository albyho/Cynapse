param(
    [Parameter(Mandatory, Position = 0)]
    [ValidateSet('new', 'remove', 'list', 'clean')]
    [string]$Action,
    [Parameter(Position = 1)]
    [string]$Name,
    [string]$Base = 'main',
    [string]$Root = (Join-Path $env:TEMP 'cynapse-worktrees'),
    [switch]$Force
)

$ErrorActionPreference = 'Stop'

function Invoke-Git {
    param([Parameter(ValueFromRemainingArguments = $true)][string[]]$Arguments)
    & git @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "git $($Arguments -join ' ') failed"
    }
}

function Get-WorktreePath {
    param([string]$Value)
    if ([string]::IsNullOrWhiteSpace($Value)) {
        throw "$Action requires a worktree name"
    }
    if ($Value -eq '.' -or $Value -eq '..' -or $Value -match '[\\/]') {
        throw "worktree name must not contain a path: $Value"
    }
    $rootPath = [IO.Path]::GetFullPath($Root)
    $path = [IO.Path]::GetFullPath((Join-Path $rootPath $Value))
    $prefix = $rootPath.TrimEnd([char[]]@(
        [IO.Path]::DirectorySeparatorChar,
        [IO.Path]::AltDirectorySeparatorChar
    )) + [IO.Path]::DirectorySeparatorChar
    if (-not $path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "worktree path escapes $rootPath`: $path"
    }
    $path
}

function Remove-Worktree {
    param([string]$Path, [switch]$Force)
    $rootPath = [IO.Path]::GetFullPath($Root)
    $fullPath = [IO.Path]::GetFullPath($Path)
    $prefix = $rootPath.TrimEnd([char[]]@(
        [IO.Path]::DirectorySeparatorChar,
        [IO.Path]::AltDirectorySeparatorChar
    )) + [IO.Path]::DirectorySeparatorChar
    if (-not $fullPath.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "refusing to remove a path outside $rootPath`: $fullPath"
    }
    $status = & git -C $fullPath status --porcelain --untracked-files=normal 2>$null
    if ($LASTEXITCODE -ne 0) {
        throw "not a git worktree: $fullPath"
    }
    if ($status -and -not $Force) {
        throw "worktree has uncommitted changes; rerun with -Force: $fullPath"
    }
    $branch = (& git -C $fullPath branch --show-current 2>$null)
    # The dirty check above is the data-loss guard; force-remove the ignored build
    # artifacts that otherwise make Windows deletion fail.
    & git worktree remove --force $fullPath 2>$null
    if ($LASTEXITCODE -ne 0) {
        Invoke-Git worktree prune
        if (Test-Path -LiteralPath $fullPath) {
            Remove-Item -LiteralPath $fullPath -Recurse -Force
        }
        if (Test-Path -LiteralPath $fullPath) {
            throw "could not remove worktree: $fullPath"
        }
    }
    Invoke-Git worktree prune
    if ($branch) {
        & git merge-base --is-ancestor $branch $Base 2>$null
        if ($LASTEXITCODE -eq 0) {
            & git branch -D $branch 2>$null | Out-Null
            if ($LASTEXITCODE -ne 0) {
                Write-Warning "could not delete merged branch ${branch}"
            }
        } else {
            # Never drop unmerged commits; the branch stays for review or merge.
            Write-Warning "kept branch ${branch}: not merged into $Base"
        }
    }
    Write-Output "removed $fullPath"
}

switch ($Action) {
    'new' {
        $path = Get-WorktreePath $Name
        $branch = "agent/$Name"
        if (Test-Path -LiteralPath $path) {
            throw "worktree already exists: $path"
        }
        New-Item -ItemType Directory -Force -Path $Root | Out-Null
        Invoke-Git worktree add -b $branch $path $Base
        Write-Output $path
    }
    'remove' {
        Remove-Worktree (Get-WorktreePath $Name) -Force:$Force
    }
    'list' {
        Invoke-Git worktree list
    }
    'clean' {
        if (Test-Path -LiteralPath $Root) {
            Get-ChildItem -LiteralPath $Root -Directory | ForEach-Object {
                Remove-Worktree $_.FullName -Force:$Force
            }
        }
        Invoke-Git worktree prune
    }
}
