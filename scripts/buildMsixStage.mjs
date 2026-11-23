//! 组装本地 MSIX 注册用的松散文件目录。
//!
//! `pnpm build:msix` 把 release 产物、JupyterLab wheel、清单模板和图标集
//! 收拢到 `packaging/msix/stage/`，之后可以直接以包身份注册并启动：
//!
//!   Add-AppxPackage -Register packaging\msix\stage\AppxManifest.xml
//!
//! 这一步只组装松散目录；要打成 `.msix` 走 `packageMsix.mjs`。

import { spawnSync } from 'node:child_process'
import {
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import path from 'node:path'
import { isMain } from './isMain.mjs'

const STAGE_DIR = path.join('packaging', 'msix', 'stage')
const TEMPLATE = path.join('packaging', 'msix', 'AppxManifest.template.xml')
const IDENTITY_FILE = path.join('packaging', 'msix', 'identity.json')
const ICON_SCRIPT = path.join('packaging', 'msix', 'makeIcons.ps1')
const ICON_SOURCE = path.join('client', 'app', 'assets', 'icons', 'icon.png')
const CLIENT_EXE = path.join('target', 'release', 'cynapse-client.exe')

const DEFAULT_IDENTITY = 'CynapseLocal.Cynapse'
const DEFAULT_PUBLISHER = 'CN=Cynapse Local Test'
const DEFAULT_PUBLISHER_DISPLAY_NAME = 'Cynapse'

/** 身份存 identity.json 并入库；环境变量优先，便于 CI 覆盖。值必须从
 *  Partner Center → Product identity 原样复制，错一个字符包就会被拒。 */
export function packageIdentity(root, env = process.env) {
  const file = path.join(root, IDENTITY_FILE)
  const stored = existsSync(file) ? JSON.parse(readFileSync(file, 'utf8')) : {}
  return {
    identity: env.MSIX_IDENTITY || stored.identity || DEFAULT_IDENTITY,
    publisher: env.MSIX_PUBLISHER || stored.publisher || DEFAULT_PUBLISHER,
    publisherDisplayName:
      env.MSIX_PUBLISHER_DISPLAY_NAME ||
      stored.publisherDisplayName ||
      DEFAULT_PUBLISHER_DISPLAY_NAME,
  }
}

function xmlAttr(value) {
  return value
    .replaceAll('&', '&amp;')
    .replaceAll('"', '&quot;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
}

/** MSIX 只认四段数字版本，每段上限 65535。`0.7.0` → `0.7.0.0`。 */
export function msixVersion(version) {
  const parts = version.split('-')[0].split('.').map(Number)
  if (parts.length > 4) {
    throw new Error(`版本号 ${version} 超过 MSIX 的四段限制`)
  }
  while (parts.length < 4) {
    parts.push(0)
  }
  for (const part of parts) {
    if (!Number.isInteger(part) || part < 0 || part > 65535) {
      throw new Error(`版本号 ${version} 无法映射为 MSIX 四段版本`)
    }
  }
  return parts.join('.')
}

function readVersion(root) {
  const manifest = JSON.parse(readFileSync(path.join(root, 'package.json'), 'utf8'))
  if (typeof manifest.version !== 'string') {
    throw new Error('package.json 缺少 version 字段')
  }
  return manifest.version
}

function copyIconSet(root, stage) {
  const result = spawnSync(
    'pwsh',
    [
      '-NoProfile',
      '-File',
      path.join(root, ICON_SCRIPT),
      '-Source',
      path.join(root, ICON_SOURCE),
      '-Destination',
      path.join(stage, 'Assets'),
    ],
    { stdio: 'inherit' },
  )
  if (result.status !== 0) {
    throw new Error('图标集生成失败，检查 ffmpeg 是否可用')
  }
}

/** 松散注册的包目录不能整目录重建：Windows 会锁住正在运行的可执行文件，
 *  本机的 safe-delete 也会拦掉批量删除。原地覆盖更省事，也顺手把「忘了
 *  退出就重新组装」这种情况变成一句能读懂的错误。 */
const LOCKED_CODES = new Set(['EBUSY', 'EPERM', 'EACCES', 'EIO'])

function copyInto(source, destination, label) {
  try {
    cpSync(source, destination)
  } catch (error) {
    if (LOCKED_CODES.has(error.code)) {
      throw new Error(
        `${label} 写不进去（${error.code}），先退出运行中的 Cynapse 再重新组装`,
      )
    }
    throw error
  }
}

export function build(root, env = process.env) {
  const version = readVersion(root)
  const stageDir = path.join(root, STAGE_DIR)

  if (!existsSync(path.join(root, CLIENT_EXE))) {
    throw new Error(
      `${CLIENT_EXE} 不存在，先运行 \`pnpm build:client\` 产出 release 可执行文件`,
    )
  }

  mkdirSync(stageDir, { recursive: true })

  copyInto(
    path.join(root, CLIENT_EXE),
    path.join(stageDir, 'cynapse-client.exe'),
    'cynapse-client.exe',
  )

  const wheel = `cynapse_jupyterlab_extension-${version}-py3-none-any.whl`
  // 同名 payload 会被覆盖，旧版本的 wheel 却会留在 stage 里一起进包。
  for (const entry of readdirSync(stageDir)) {
    if (entry.endsWith('.whl') && entry !== wheel) {
      rmSync(path.join(stageDir, entry), { force: true })
    }
  }
  const wheelPath = path.join(root, 'adapters', 'jupyterlab', 'dist', wheel)
  if (existsSync(wheelPath)) {
    copyInto(wheelPath, path.join(stageDir, wheel), wheel)
  } else {
    console.warn(
      `警告: 缺少 ${path.join('adapters', 'jupyterlab', 'dist', wheel)}，` +
        'JupyterLab 功能在本次验证中不可用（运行 `pnpm build:jupyterlab` 可补齐）',
    )
  }

  const version4 = msixVersion(version)
  const { identity, publisher, publisherDisplayName } = packageIdentity(root, env)
  const template = readFileSync(path.join(root, TEMPLATE), 'utf8')
  writeFileSync(
    path.join(stageDir, 'AppxManifest.xml'),
    template
      .split('__VERSION__').join(version4)
      .split('__IDENTITY__').join(xmlAttr(identity))
      .split('__PUBLISHER__').join(xmlAttr(publisher))
      .split('__PUBLISHER_DISPLAY_NAME__').join(xmlAttr(publisherDisplayName)),
  )

  copyIconSet(root, stageDir)

  return { stageDir, version, version4, identity, publisher }
}

function main() {
  const { stageDir, version, version4, identity } = build(process.cwd())
  console.log(`staged Cynapse ${version} as MSIX version ${version4}`)
  console.log(`  identity: ${identity}`)
  console.log(`  ${path.join(STAGE_DIR, 'AppxManifest.xml')}`)
  console.log('')
  console.log('本地注册（需要开发者模式或旁加载已启用）:')
  console.log(`  Add-AppxPackage -Register ${path.join(STAGE_DIR, 'AppxManifest.xml')}`)
  console.log('启动: 从开始菜单搜索 "Cynapse"')
  console.log(`移除注册:  Get-AppxPackage ${identity} | Remove-AppxPackage`)
  if (identity === DEFAULT_IDENTITY) {
    console.warn('提示: 当前是本地占位身份，提交商店前设置 MSIX_IDENTITY / MSIX_PUBLISHER')
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
