import { execFileSync, spawnSync } from 'node:child_process'
import path from 'node:path'

const adapterRoot = path.resolve(import.meta.dirname, '..')
const repoRoot = path.resolve(adapterRoot, '../..')
const pathspecs = ['adapters/vscode/src', 'adapters/vscode/l10n']
const files = new Set()

function collect(args) {
  try {
    const output = execFileSync('git', ['-C', repoRoot, ...args], {
      encoding: 'utf8',
    })
    for (const file of output.trim().split(/\r?\n/)) {
      if (file) {
        files.add(file)
      }
    }
  } catch {
    // A shallow checkout may not have the comparison base; the other
    // working-tree checks still apply.
  }
}

if (process.env.GITHUB_BASE_REF) {
  collect([
    'diff',
    '--name-only',
    '--diff-filter=ACMRTUXB',
    `origin/${process.env.GITHUB_BASE_REF}...HEAD`,
    '--',
    ...pathspecs,
  ])
}
collect(['diff', '--name-only', '--diff-filter=ACMRTUXB', 'HEAD', '--', ...pathspecs])
collect(['ls-files', '--others', '--exclude-standard', '--', ...pathspecs])

const relativeFiles = [...files]
  .map((file) => path.relative(adapterRoot, path.join(repoRoot, file)))
  .filter((file) => /\.(ts|json)$/.test(file))
  .sort()

if (relativeFiles.length === 0) {
  console.log('no changed Adapter source files to format-check')
  process.exit(0)
}

const result = spawnSync(
  'pnpm',
  ['exec', 'prettier', '--check', ...relativeFiles],
  { cwd: adapterRoot, stdio: 'inherit', shell: true },
)
process.exit(result.status ?? 1)
