import { readFileSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import { isMain } from './isMain.mjs'

const FILES = [
  { path: 'Cargo.toml', kind: 'version-line' },
  { path: 'package.json', kind: 'json' },
  { path: 'adapters/vscode/package.json', kind: 'json' },
  { path: 'adapters/jupyterlab/package.json', kind: 'json' },
  { path: 'adapters/jupyterlab/pyproject.toml', kind: 'version-line' },
]

const VERSION = /^\d+\.\d+\.\d+$/

function versionLine(text) {
  const matches = [...text.matchAll(/^version = "([^"]+)"/gm)]
  if (matches.length !== 1) {
    throw new Error('version-line file must have exactly one version line')
  }
  return matches[0][1]
}

function readVersion(root, file) {
  const text = readFileSync(path.join(root, file.path), 'utf8')
  return file.kind === 'json' ? JSON.parse(text).version : versionLine(text)
}

export function checkVersions(root) {
  const versions = FILES.map((file) => ({
    path: file.path,
    version: readVersion(root, file),
  }))
  if (new Set(versions.map(({ version }) => version)).size !== 1) {
    throw new Error(
      versions.map(({ path, version }) => `${path}: ${version}`).join('\n'),
    )
  }
  return versions[0].version
}

export function setVersion(root, version) {
  if (!VERSION.test(version)) {
    throw new Error(`Invalid version: ${version}`)
  }
  const updates = FILES.map((file) => {
    const target = path.join(root, file.path)
    const text = readFileSync(target, 'utf8')
    const next =
      file.kind === 'json'
        ? text.replace(/^(\s*"version":\s*")[^"]+(")/m, `$1${version}$2`)
        : text.replace(/^version = "[^"]+"/m, `version = "${version}"`)
    if (next === text) {
      throw new Error(`${file.path} has no version field to update`)
    }
    return { target, next }
  })
  for (const { target, next } of updates) {
    writeFileSync(target, next)
  }
  return version
}

function main() {
  if (process.argv[2] === '--check') {
    try {
      console.log(checkVersions(process.cwd()))
    } catch (error) {
      console.error(error.message)
      process.exitCode = 1
    }
    return
  }

  try {
    setVersion(process.cwd(), process.argv[2])
  } catch (error) {
    console.error(error.message)
    console.error('usage: node scripts/setVersion.mjs <major.minor.patch>')
    process.exitCode = 1
    return
  }

  console.log(`Version set to ${process.argv[2]} in:`)
  for (const file of FILES) {
    console.log(`  ${file.path}`)
  }
  console.log('Run `cargo update -w` to refresh Cargo.lock.')
}

if (isMain(import.meta.url)) {
  main()
}
