//! 把 stage 目录打成本机或 CI 都能用的 `.msix` 包。
//!
//! `pnpm package:msix` 是本地打包路径；公开仓库的
//! `.github/workflows/release.yml` 调的是同一条命令，两边产物完全一致。产出的
//! 是**未签名**包：Microsoft Store 提交时会自己重签，本机安装测试才需要另外签名。

import { spawnSync } from 'node:child_process'
import { existsSync, mkdirSync, readdirSync, statSync } from 'node:fs'
import path from 'node:path'
import { build } from './buildMsixStage.mjs'
import { isMain } from './isMain.mjs'

const OUT_DIR = path.join('dist', 'msix')
const DEFAULT_IDENTITY = 'CynapseLocal.Cynapse'
const SDK_BIN_ROOTS = [
  ...(process.env.WindowsSdkDir ? [process.env.WindowsSdkDir] : []),
  'C:\\Program Files (x86)\\Windows Kits\\10\\bin',
  'C:\\Program Files\\Windows Kits\\10\\bin',
]

/** 选本机安装的最高版本 SDK，避免把路径写死到某个 Windows 版本号。 */
export function findMakeAppx() {
  for (const root of SDK_BIN_ROOTS) {
    if (!existsSync(root)) {
      continue
    }
    const versions = readdirSync(root, { withFileTypes: true })
      .filter(
        (entry) =>
          entry.isDirectory() && /^\d+\.\d+\.\d+\.\d+$/.test(entry.name),
      )
      .map((entry) => entry.name)
      .sort((a, b) => a.localeCompare(b, undefined, { numeric: true }))
      .reverse()

    for (const version of versions) {
      const candidate = path.join(root, version, 'x64', 'makeappx.exe')
      if (existsSync(candidate)) {
        return candidate
      }
    }
  }
  throw new Error(
    '找不到 makeappx.exe；安装 Windows SDK 的「Windows 应用打包工具」后重试',
  )
}

export function packageMsix(root, env = process.env) {
  const staged = build(root, env)
  const makeappx = findMakeAppx()
  const outDir = path.join(root, OUT_DIR)
  mkdirSync(outDir, { recursive: true })

  // 文件名是任意的，不影响包身份（身份只由 manifest 的 Identity 决定）。
  // 固定用产品名做前缀，免得商店身份换掉后文件名跟着变。
  const output = path.join(outDir, `CynapseClient_${staged.version4}_x64.msix`)
  const result = spawnSync(
    makeappx,
    ['pack', '/d', staged.stageDir, '/p', output, '/o'],
    { stdio: 'inherit' },
  )
  if (result.status !== 0) {
    throw new Error('makeappx pack 失败')
  }
  if (!existsSync(output)) {
    throw new Error(`makeappx 没有产出 ${output}`)
  }
  return { output, ...staged }
}

function main() {
  const { output, version, version4, identity } = packageMsix(process.cwd())
  const mb = (statSync(output).size / 1024 / 1024).toFixed(1)
  console.log(
    `packed Cynapse ${version} → ${path.join(OUT_DIR, path.basename(output))} (${mb} MB)`,
  )
  console.log(`  MSIX version: ${version4}; identity: ${identity}`)
  console.log('  未签名；Store 提交可直接上传，本机安装测试需另行签名')
  if (identity === DEFAULT_IDENTITY) {
    console.warn('提示: 占位身份不能提交商店，设置 MSIX_IDENTITY / MSIX_PUBLISHER 后重跑')
  }
}

if (isMain(import.meta.url)) {
  try {
    main()
  } catch (error) {
    console.error(error.message)
    process.exitCode = 1
  }
}
