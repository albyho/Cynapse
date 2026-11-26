import { execFileSync } from 'node:child_process'
import { readdirSync, readFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const MIN_LENGTH = 20
const CLIENT_ROOT = 'client'

export function removedLocaleValues(diff) {
  const values = []
  for (const line of diff.split('\n')) {
    if (!line.startsWith('-') || line.startsWith('---')) continue
    const match = line.match(/^-\s*([\w.-]+):\s*"(.*)"\s*$/)
    if (!match) continue
    const [, key, value] = match
    if (value.length >= MIN_LENGTH) values.push({ key, value })
  }
  return values
}

function rustFiles(dir) {
  const files = []
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name)
    if (entry.isDirectory()) files.push(...rustFiles(full))
    else if (entry.isFile() && entry.name.endsWith('.rs')) files.push(full)
  }
  return files
}

export function findStaleLiterals(values, files) {
  const findings = []
  for (const { key, value } of values) {
    for (const file of files) {
      const lines = readFileSync(file, 'utf8').split('\n')
      lines.forEach((line, index) => {
        if (line.includes(value)) {
          findings.push({ key, value, file, line: index + 1 })
        }
      })
    }
  }
  return findings
}

function main() {
  const diff = execFileSync('git', ['diff', 'HEAD', '--', 'client/i18n/locales'], {
    encoding: 'utf8',
  })
  const values = removedLocaleValues(diff)
  if (values.length === 0) return

  const findings = findStaleLiterals(values, rustFiles(CLIENT_ROOT))
  if (findings.length === 0) return

  for (const finding of findings) {
    console.error(
      `stale i18n literal for ${finding.key} in ${finding.file}:${finding.line}: ${finding.value}`
    )
  }
  console.error('Update the hard-coded literal to the new copy.')
  process.exit(1)
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main()
}
