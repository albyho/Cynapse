import { execFileSync } from 'node:child_process'
import { isMain } from './isMain.mjs'

// Keep in sync with the trigger list in docs/agents/review.md.
const REVIEW_TRIGGERS = [
  /^client\/(core|model|harness)\/src\//,
  /^client\/app\/src\/(main|settings)\.rs$/,
  /^client\/app\/src\/platform\//,
  /^course\/src\//,
  /^adapters\/(adapter-kit|vscode|jupyterlab)\/src\//,
  /^protocol\//,
  /^spec\.md$/,
]

// Test-only files cannot change a public API, persisted state, or startup path.
const REVIEW_EXCLUDES = [
  /^client\/(core|model|harness)\/src\/(?:.*\/)?tests\//,
  /^client\/(core|model|harness)\/src\/(?:.*\/)?tests\.rs$/,
]

export function reviewTriggerPaths(files) {
  return files.filter(
    (file) =>
      !REVIEW_EXCLUDES.some((pattern) => pattern.test(file)) &&
      REVIEW_TRIGGERS.some((pattern) => pattern.test(file)),
  )
}

function gitFiles(args) {
  const output = execFileSync('git', args, { encoding: 'utf8' })
  return output.trim().split(/\r?\n/).filter(Boolean)
}

function main() {
  const staged = process.argv.includes('--staged')
  const files = staged
    ? [...new Set(gitFiles(['diff', '--cached', '--name-only', 'HEAD']))]
    : [
        ...new Set([
          ...gitFiles(['diff', '--name-only', 'HEAD']),
          ...gitFiles(['ls-files', '--others', '--exclude-standard']),
        ]),
      ]

  if (files.length === 0) {
    console.log(
      staged
        ? 'No staged changes to review.'
        : 'No working-tree changes to review.',
    )
    return
  }

  const stat = staged
    ? ['diff', '--cached', '--stat', 'HEAD']
    : ['diff', '--stat', 'HEAD']
  console.log(execFileSync('git', stat, { encoding: 'utf8' }))
  console.log('Changed files:')
  for (const file of files) {
    console.log(`- ${file}`)
  }

  const triggers = reviewTriggerPaths(files)
  if (triggers.length > 0) {
    console.log('Review gate trigger paths:')
    for (const file of triggers) {
      console.log(`- ${file}`)
    }
    if (process.env.REVIEW_CONFIRMED === '1') {
      console.log('Review gate confirmed (REVIEW_CONFIRMED=1).')
      return
    }
    if (process.env.SKIP_REVIEW === '1') {
      console.log('Review gate skipped (SKIP_REVIEW=1).')
      return
    }
    const strict = process.argv.includes('--strict')
    const branch = execFileSync('git', ['rev-parse', '--abbrev-ref', 'HEAD'], {
      encoding: 'utf8',
    }).trim()
    if (strict && branch === 'main') {
      console.error(
        'Review gate required on main. Run the review gate in ' +
          'docs/agents/review.md, then set REVIEW_CONFIRMED=1; ' +
          'set SKIP_REVIEW=1 only to skip the review.',
      )
      process.exit(1)
    }
    console.log('Run the review gate in docs/agents/review.md before committing.')
  }
}

if (isMain(import.meta.url)) {
  main()
}
