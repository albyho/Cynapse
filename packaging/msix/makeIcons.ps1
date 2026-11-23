#Requires -Version 7.0
<#
.SYNOPSIS
  从一张方形源图生成 MSIX 清单要求的固定尺寸图标集。

.DESCRIPTION
  尺寸取自 Microsoft Store 的包资源要求。只用 ffmpeg 缩放，不引入新的
  Node 或 .NET 依赖。非方形规格（Wide310x150）按等比缩放后透明留边，
  不拉伸变形。
#>
param(
  [Parameter(Mandatory = $true)][string]$Source,
  [Parameter(Mandatory = $true)][string]$Destination
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path $Source)) {
  throw "源图不存在: $Source"
}

$ffmpeg = (Get-Command ffmpeg -ErrorAction SilentlyContinue).Source
if (-not $ffmpeg) {
  throw '找不到 ffmpeg，请确认它在 PATH 中'
}

New-Item -ItemType Directory -Force -Path $Destination | Out-Null

$specs = @(
  @{ Name = 'StoreLogo.png';                       Width = 50;  Height = 50  },
  @{ Name = 'Square44x44Logo.png';                 Width = 44;  Height = 44  },
  @{ Name = 'Square71x71Logo.png';                 Width = 71;  Height = 71  },
  @{ Name = 'Square150x150Logo.png';               Width = 150; Height = 150 },
  @{ Name = 'Square310x310Logo.png';               Width = 310; Height = 310 },
  @{ Name = 'Wide310x150Logo.png';                 Width = 310; Height = 150 },
  @{ Name = 'Square44x44Logo.targetsize-16.png';   Width = 16;  Height = 16  },
  @{ Name = 'Square44x44Logo.targetsize-24.png';   Width = 24;  Height = 24  },
  @{ Name = 'Square44x44Logo.targetsize-32.png';   Width = 32;  Height = 32  },
  @{ Name = 'Square44x44Logo.targetsize-48.png';   Width = 48;  Height = 48  },
  @{ Name = 'Square44x44Logo.targetsize-256.png';  Width = 256; Height = 256 }
)

foreach ($spec in $specs) {
  $output = Join-Path $Destination $spec.Name
  $size = "$($spec.Width):$($spec.Height)"
  if ($spec.Width -eq $spec.Height) {
    $filter = "scale=$size`:flags=lanczos"
  } else {
    $filter = "scale=$size`:force_original_aspect_ratio=decrease:flags=lanczos,pad=$size`:(ow-iw)/2:(oh-ih)/2:color=black@0"
  }
  & $ffmpeg -y -loglevel error -i $Source -vf $filter -pix_fmt rgba $output
  if ($LASTEXITCODE -ne 0) {
    throw "生成 $($spec.Name) 失败"
  }
}

Write-Output "generated $($specs.Count) icons in $Destination"
