import { execFileSync, spawnSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

import { isMain } from './isMain.mjs'

const CONFIG = fileURLToPath(new URL('../.rumdl.toml', import.meta.url))

export function markdownPaths(files) {
  return files.filter((file) => file.toLowerCase().endsWith('.md'))
}

export function parseArgs(argv) {
  const options = { mode: 'all', paths: [], help: false }
  for (const arg of argv) {
    if (arg === '--all') options.mode = 'all'
    else if (arg === '--staged') options.mode = 'staged'
    else if (arg === '--help' || arg === '-h') options.help = true
    else options.paths.push(arg)
  }
  return options
}

function gitFiles(args) {
  return execFileSync('git', args, { encoding: 'utf8' })
    .split(/\r?\n/)
    .filter(Boolean)
}

export function selectFiles(options) {
  if (options.paths.length > 0) return options.paths
  if (options.mode === 'all') return markdownPaths(gitFiles(['ls-files', '*.md']))
  return markdownPaths(
    gitFiles(['diff', '--cached', '--name-only', '--diff-filter=ACMR']),
  )
}

export function findRumdl() {
  if (process.env.RUMDL) return process.env.RUMDL
  const executable = process.platform === 'win32' ? 'rumdl.exe' : 'rumdl'
  const local = join(homedir(), '.local', 'bin', executable)
  if (existsSync(local)) return local
  const probe = spawnSync('rumdl', ['--version'], { stdio: 'ignore' })
  return probe.error || probe.status !== 0 ? null : 'rumdl'
}

function main() {
  const options = parseArgs(process.argv.slice(2))
  if (options.help) {
    console.log('Usage: node scripts/checkMarkdown.mjs [--all|--staged] [paths...]')
    console.log('  --all     lint every tracked Markdown file (default)')
    console.log('  --staged  lint staged Markdown files only')
    return 0
  }

  const files = selectFiles(options)
  if (files.length === 0) {
    if (options.mode !== 'all') console.log('No staged Markdown files.')
    return 0
  }

  const rumdl = findRumdl()
  if (!rumdl) {
    console.log('rumdl not found; skipping Markdown lint.')
    return 0
  }

  const result = spawnSync(
    rumdl,
    ['check', '--config', CONFIG, '--color', 'never', ...files],
    { stdio: 'inherit' },
  )
  return result.status ?? 1
}

if (isMain(import.meta.url)) {
  process.exitCode = main()
}
