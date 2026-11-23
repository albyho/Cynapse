#!/usr/bin/env node
// Starts the Client with a locally built JupyterLab extension wheel so manual
// tests do not need to set CYNAPSE_EXTENSION_WHEEL by hand.
import { existsSync, readdirSync, statSync } from 'node:fs'
import { spawnSync } from 'node:child_process'
import path from 'node:path'
import { isMain } from './isMain.mjs'

const WHEEL = /^cynapse_jupyterlab_extension-(.+)-py3-none-any\.whl$/
const ADAPTER = path.resolve('adapters', 'jupyterlab')
const DIST = path.join(ADAPTER, 'dist')
const SOURCE_PATHS = [
  path.join(ADAPTER, 'src'),
  path.join(ADAPTER, 'package.json'),
  path.join(ADAPTER, 'tsconfig.json'),
  path.join(ADAPTER, 'pyproject.toml'),
  path.join(ADAPTER, 'scripts', 'buildExtension.mjs'),
]

function latestWheel() {
  if (!existsSync(DIST)) {
    return undefined
  }
  return readdirSync(DIST)
    .filter((name) => WHEEL.test(name))
    .map((name) => path.join(DIST, name))
    .sort((left, right) => statSync(right).mtimeMs - statSync(left).mtimeMs)[0]
}

function wheelVersion(wheel) {
  return WHEEL.exec(path.basename(wheel))?.[1]
}

function newestMtime(target) {
  const stat = statSync(target)
  if (!stat.isDirectory()) {
    return stat.mtimeMs
  }
  let newest = 0
  for (const entry of readdirSync(target, { withFileTypes: true })) {
    newest = Math.max(newest, newestMtime(path.join(target, entry.name)))
  }
  return newest
}

function wheelIsStale(wheel) {
  const wheelMtime = statSync(wheel).mtimeMs
  return SOURCE_PATHS.some(
    (source) => existsSync(source) && newestMtime(source) > wheelMtime,
  )
}

function buildJupyterLab() {
  const result = spawnSync('pnpm', ['build:jupyterlab'], { stdio: 'inherit' })
  if (result.error) {
    console.error(`Failed to run pnpm: ${result.error.message}`)
    process.exit(1)
  }
  if (result.status !== 0) {
    process.exit(result.status ?? 1)
  }
}

function main() {
  const args = process.argv.slice(2)
  const build = args.includes('--build')
  const clientArgs = args.filter((argument) => argument !== '--build')
  let wheel = process.env.CYNAPSE_EXTENSION_WHEEL

  if (!wheel) {
    wheel = latestWheel()
    if (!wheel || build || wheelIsStale(wheel)) {
      if (!build && wheel) {
        console.log('Adapter sources are newer than the wheel; rebuilding...')
      }
      buildJupyterLab()
      wheel = latestWheel()
    }
  }

  if (!wheel) {
    console.error(
      'No JupyterLab extension wheel found. Run `pnpm build:jupyterlab` first.',
    )
    process.exit(1)
  }
  wheel = path.resolve(wheel)

  const env = { ...process.env, CYNAPSE_EXTENSION_WHEEL: wheel }
  const version = wheelVersion(wheel)
  if (version && !env.CYNAPSE_EXTENSION_VERSION) {
    env.CYNAPSE_EXTENSION_VERSION = version
  }

  console.log(`Using JupyterLab extension: ${wheel}`)
  if (clientArgs.length === 0) {
    console.log('No Client arguments; select a JupyterLab course in the Client.')
  }

  const result = spawnSync(
    'cargo',
    ['run', '-p', 'cynapse-client', '--', ...clientArgs],
    { stdio: 'inherit', env },
  )
  if (result.error) {
    console.error(`Failed to run cargo: ${result.error.message}`)
    process.exit(1)
  }
  process.exit(result.status ?? 1)
}

if (isMain(import.meta.url)) {
  main()
}
