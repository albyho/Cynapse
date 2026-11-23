import { existsSync, readFileSync } from 'node:fs'
import path from 'node:path'
import { isMain } from './isMain.mjs'

const PNPM_BUILTINS = new Set([
  'add',
  'audit',
  'bin',
  'config',
  'create',
  'deploy',
  'dlx',
  'doctor',
  'env',
  'exec',
  'fetch',
  'i',
  'import',
  'init',
  'install',
  'licenses',
  'link',
  'list',
  'ls',
  'outdated',
  'pack',
  'patch',
  'patch-commit',
  'patch-remove',
  'prune',
  'publish',
  'rebuild',
  'remove',
  'rm',
  'root',
  'run',
  'server',
  'setup',
  'store',
  'unlink',
  'up',
  'update',
  'why',
])

export function extractPnpmCommands(markdown) {
  const commands = new Set()

  for (const block of markdown.matchAll(/```[^\n]*\r?\n([\s\S]*?)```/g)) {
    for (const line of block[1].split(/\r?\n/)) {
      const tokens = line.trim().split(/\s+/)
      if (tokens[0] !== 'pnpm') {
        continue
      }

      let name = tokens[1]
      if (name === 'run') {
        name = tokens[2]
      }
      if (!name || name.startsWith('-') || PNPM_BUILTINS.has(name)) {
        continue
      }
      commands.add(name)
    }
  }

  return [...commands].sort()
}

export function extractRustVersion(markdown) {
  return markdown.match(/Rust\s+(\d+\.\d+)(?:\.\d+)?\+/i)?.[1]
}

export function extractScriptTable(markdown) {
  return [
    ...new Set(
      [...markdown.matchAll(/^\| `([a-z][a-z0-9:-]*)` \|/gm)].map(
        (match) => match[1],
      ),
    ),
  ].sort()
}

function isGateScript(name) {
  return (
    name === 'verify' || name.startsWith('verify:') || name.startsWith('review:')
  )
}

export function checkScriptReference(scripts, markdown) {
  const documented = new Set(extractScriptTable(markdown))
  const errors = []
  for (const name of Object.keys(scripts)) {
    if (isGateScript(name) && !documented.has(name)) {
      errors.push(`pnpm script reference: missing pnpm ${name}`)
    }
  }
  for (const name of documented) {
    if (!(name in scripts)) {
      errors.push(`pnpm script reference: pnpm ${name} is not a package script`)
    }
  }
  return errors
}

function majorMinor(version) {
  return version.split('.').slice(0, 2).join('.')
}

export function checkDocs(root) {
  const packageJson = JSON.parse(
    readFileSync(path.join(root, 'package.json'), 'utf8'),
  )
  const scripts = packageJson.scripts ?? {}
  const cargoToml = readFileSync(
    path.join(root, 'client/app/Cargo.toml'),
    'utf8',
  )
  const rustVersion = cargoToml.match(/rust-version\s*=\s*"([^"]+)"/)?.[1]
  const errors = []

  if (!rustVersion) {
    errors.push('client/app/Cargo.toml has no rust-version')
  }

  for (const document of ['README.md', 'README.zh-CN.md']) {
    const markdown = readFileSync(path.join(root, document), 'utf8')
    for (const command of extractPnpmCommands(markdown)) {
      if (!(command in scripts)) {
        errors.push(`${document}: pnpm ${command} is not a package script`)
      }
    }

    const documentedRust = extractRustVersion(markdown)
    if (!documentedRust) {
      errors.push(`${document}: missing Rust version requirement`)
    } else if (rustVersion && majorMinor(documentedRust) !== majorMinor(rustVersion)) {
      errors.push(
        `${document}: requires Rust ${documentedRust}, but Cargo requires ${rustVersion}`,
      )
    }
  }

  const referencePath = path.join(
    root,
    'docs',
    'pnpm脚本参考(pnpm-scripts).md',
  )
  if (existsSync(referencePath)) {
    errors.push(
      ...checkScriptReference(scripts, readFileSync(referencePath, 'utf8')),
    )
  }

  return errors
}

function main() {
  const errors = checkDocs(process.cwd())
  if (errors.length === 0) {
    console.log('documentation commands and Rust versions OK')
    return
  }

  for (const error of errors) {
    console.error(error)
  }
  process.exitCode = 1
}

if (isMain(import.meta.url)) {
  main()
}
