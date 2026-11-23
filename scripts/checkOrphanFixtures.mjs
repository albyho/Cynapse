import { readdirSync, readFileSync } from 'node:fs'
import path from 'node:path'
import { isMain } from './isMain.mjs'

const FIXTURE_ROOTS = ['course/test-data', 'client/test-data', 'protocol/test-data']
const DYNAMIC_FIXTURE_ROOTS = new Set(['protocol/test-data'])
const REFERENCE_ROOTS = [
  'adapters',
  'client',
  'course',
  'docs',
  'protocol',
  'scripts',
  'tools',
]
const REFERENCE_FILES = [
  'AGENTS.md',
  'GLOSSARY.md',
  'package.json',
  'spec.md',
]
const TEXT_EXTENSIONS = new Set([
  '.json',
  '.md',
  '.mjs',
  '.ps1',
  '.py',
  '.rs',
  '.toml',
  '.ts',
  '.tsx',
  '.yml',
  '.yaml',
])
const SKIPPED_DIRECTORIES = new Set([
  '.git',
  '.vscode-test',
  'node_modules',
  'out',
  'target',
])

function walk(directory, visit) {
  let entries
  try {
    entries = readdirSync(directory, { withFileTypes: true })
  } catch (error) {
    if (error?.code === 'ENOENT') {
      return
    }
    throw error
  }
  for (const entry of entries) {
    const file = path.join(directory, entry.name)
    if (entry.isDirectory()) {
      if (!SKIPPED_DIRECTORIES.has(entry.name)) {
        walk(file, visit)
      }
    } else if (TEXT_EXTENSIONS.has(path.extname(entry.name))) {
      visit(file)
    }
  }
}

function isInside(root, file, directory) {
  const relative = path.relative(path.join(root, directory), file)
  return relative !== '' && !relative.startsWith('..') && !path.isAbsolute(relative)
}

export function listFixtureEntries(root) {
  const entries = []
  for (const fixtureRoot of FIXTURE_ROOTS) {
    if (DYNAMIC_FIXTURE_ROOTS.has(fixtureRoot)) {
      continue
    }
    const directory = path.join(root, fixtureRoot)
    let children
    try {
      children = readdirSync(directory, { withFileTypes: true })
    } catch (error) {
      if (error?.code === 'ENOENT') {
        continue
      }
      throw error
    }
    for (const child of children) {
      if (child.name !== 'README.md') {
        entries.push(path.posix.join(fixtureRoot, child.name))
      }
    }
  }
  return entries.sort()
}

export function findOrphanFixtures(root) {
  const references = []
  for (const directory of REFERENCE_ROOTS) {
    walk(path.join(root, directory), (file) => {
      // Fixture contents cannot prove that the fixture itself is used.
      if (FIXTURE_ROOTS.some((fixtureRoot) => isInside(root, file, fixtureRoot))) {
        return
      }
      references.push(readFileSync(file, 'utf8'))
    })
  }
  for (const file of REFERENCE_FILES) {
    try {
      references.push(readFileSync(path.join(root, file), 'utf8'))
    } catch (error) {
      if (error?.code !== 'ENOENT') {
        throw error
      }
    }
  }

  return listFixtureEntries(root).filter((entry) => {
    const name = path.basename(entry)
    return !references.some((text) => text.includes(name))
  })
}

function main() {
  const orphans = findOrphanFixtures(process.cwd())
  if (orphans.length === 0) {
    console.log('all test fixtures are referenced')
    return
  }
  console.log('Unreferenced test fixtures (report only):')
  for (const entry of orphans) {
    console.log(`  ${entry}`)
  }
  console.log('protocol/test-data is skipped because its files are discovered dynamically.')
}

if (isMain(import.meta.url)) {
  main()
}
