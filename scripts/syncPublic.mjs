#!/usr/bin/env node
// Copies the publishable part of this repository into the public working copy.
//
// The include list is the only source of truth: a path that is not matched here
// is never copied, so a new internal document cannot reach the public tree just
// by being added to the repository. `README.md`, `README.zh-CN.md`,
// `.gitignore` and `.github/` belong to the public repository and are never
// touched -- a shared `.gitignore` would publish the private directory layout.

import { execFileSync } from 'node:child_process'
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { isMain } from './isMain.mjs'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const INCLUDED = [
  'client',
  'adapters',
  'protocol',
  'course',
  'packaging',
  'scripts',
  'Cargo.toml',
  'Cargo.lock',
  'package.json',
  'pnpm-lock.yaml',
  'pnpm-workspace.yaml',
  '.gitattributes',
  '.rumdl.toml',
  'LICENSE',
  'CHANGELOG.md',
]

// Inside an included directory, these stay private.
const EXCLUDED = [
  /^adapters\/scratch\/research\//,
  /^packaging\/msix\/store-submission\.md$/,
]

export function publishedPath(file) {
  const included = INCLUDED.some(
    (entry) => file === entry || file.startsWith(`${entry}/`),
  )
  return included && !EXCLUDED.some((pattern) => pattern.test(file))
}

function trackedFiles(repository) {
  const output = execFileSync('git', ['-C', repository, 'ls-files', '-z'], {
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
  })
  return output.split('\0').filter(Boolean)
}

/**
 * Compares the published subset of `source` with `target`. Returns the files to
 * copy (added or modified) and the files to delete.
 */
export function planSync(source, target) {
  const wanted = trackedFiles(source).filter(publishedPath)
  const wantedSet = new Set(wanted)
  const added = []
  const modified = []
  for (const file of wanted) {
    const from = path.join(source, file)
    const to = path.join(target, file)
    if (!existsSync(to)) {
      added.push(file)
      continue
    }
    if (!readFileSync(from).equals(readFileSync(to))) {
      modified.push(file)
    }
  }
  const removed = trackedFiles(target)
    .filter(publishedPath)
    .filter((file) => !wantedSet.has(file))
    // A file already gone from disk is only waiting for `git add -A`, which is
    // the operator's job; counting it again would make --check never converge.
    .filter((file) => existsSync(path.join(target, file)))
    .sort()
  return { added: added.sort(), modified, removed }
}

function parseArgs(argv) {
  const options = { target: undefined, check: false, help: false }
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index]
    if (arg === '--target') {
      options.target = argv[index + 1]
      index += 1
    } else if (arg === '--check') {
      options.check = true
    } else if (arg === '--help' || arg === '-h') {
      options.help = true
    } else {
      throw new Error(`unknown argument: ${arg}`)
    }
  }
  return options
}

function main() {
  const options = parseArgs(process.argv.slice(2))
  if (options.help || !options.target) {
    console.log(
      'Usage: node scripts/syncPublic.mjs --target <public-checkout> [--check]',
    )
    console.log('  --check  report what would change; exit 1 when it is not empty')
    return options.help ? 0 : 1
  }

  const target = path.resolve(options.target)
  if (!existsSync(path.join(target, '.git'))) {
    console.error(`Not a git checkout: ${target}`)
    return 1
  }

  const plan = planSync(ROOT, target)
  const changes = plan.added.length + plan.modified.length + plan.removed.length
  for (const file of plan.added) console.log(`+ ${file}`)
  for (const file of plan.modified) console.log(`~ ${file}`)
  for (const file of plan.removed) console.log(`- ${file}`)
  console.log(
    `${plan.added.length} added, ${plan.modified.length} modified, ${plan.removed.length} deleted`,
  )

  if (options.check) {
    return changes === 0 ? 0 : 1
  }
  if (changes === 0) {
    return 0
  }

  for (const file of [...plan.added, ...plan.modified]) {
    const to = path.join(target, file)
    mkdirSync(path.dirname(to), { recursive: true })
    copyFileSync(path.join(ROOT, file), to)
  }
  for (const file of plan.removed) {
    rmSync(path.join(target, file), { force: true })
  }
  stage(target, [...plan.added, ...plan.modified, ...plan.removed])
  console.log(`Synced into ${target}. Review the staged diff, then commit there.`)
  return 0
}

/**
 * Stages exactly the paths this script touched. A plain `git add -A` is not
 * enough: a global gitignore (for example `*.ogg`) silently drops tracked
 * files, and it can also sweep in something that is not part of the whitelist.
 */
function stage(target, files) {
  const list = files.join('\0')
  execFileSync(
    'git',
    [
      '-C',
      target,
      'add',
      '-A',
      '-f',
      '--pathspec-from-file=-',
      '--pathspec-file-nul',
    ],
    { input: `${list}\0`, stdio: ['pipe', 'inherit', 'inherit'] },
  )
}

if (isMain(import.meta.url)) {
  process.exitCode = main()
}
