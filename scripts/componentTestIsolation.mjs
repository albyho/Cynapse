import { readdir, readFile } from 'node:fs/promises'
import * as path from 'node:path'
import { isMain } from './isMain.mjs'

const sourceExtensions = new Set([
  '.js',
  '.mjs',
  '.rs',
  '.ts',
  '.tsx',
  '.vue',
])
const excludedDirectories = new Set([
  '.vscode-test',
  'dist',
  'gen',
  'node_modules',
  'out',
  'target',
  'test-data',
])

export function findForbiddenReferences(source, component = '') {
  const findings = []
  const lines = source.replaceAll('\r\n', '\n').split('\n')

  for (const [index, rawLine] of lines.entries()) {
    if (rawLine.includes('test-isolation-allow:')) {
      continue
    }
    const line = rawLine.replaceAll('\\', '/')
    const relativePattern = /(?:\.\.\/)+courses\//g
    const relativeMatches = [...line.matchAll(relativePattern)]
    for (const match of relativeMatches) {
      findings.push({ line: index + 1, reference: match[0] })
    }

    const withoutRelative = line.replace(relativePattern, '')
    const directPattern = /(?<![A-Za-z0-9_])courses\//g
    for (const match of withoutRelative.matchAll(directPattern)) {
      findings.push({ line: index + 1, reference: match[0] })
    }

    for (const match of line.matchAll(/["'`](courses)["'`]/g)) {
      findings.push({ line: index + 1, reference: match[1] })
    }

    if (component === 'course') {
      for (const match of line.matchAll(/(?:\.\.\/)*client\/test-data\//g)) {
        findings.push({ line: index + 1, reference: match[0] })
      }
    }

    for (const pattern of [
      /(?<![A-Za-z0-9_])[A-Za-z]:\//g,
      /\/tmp\//g,
      /file:\/\/\//g,
    ]) {
      for (const match of line.matchAll(pattern)) {
        findings.push({ line: index + 1, reference: match[0] })
      }
    }
  }

  return findings
}

async function directoryNames(directory) {
  try {
    const entries = await readdir(directory, { withFileTypes: true })
    return entries.filter((entry) => entry.isDirectory()).map((entry) => entry.name)
  } catch (error) {
    if (error?.code === 'ENOENT') {
      return []
    }
    throw error
  }
}

export async function findDuplicateFixtureNames(root) {
  const [course, client] = await Promise.all([
    directoryNames(path.join(root, 'course/test-data')),
    directoryNames(path.join(root, 'client/test-data')),
  ])
  const clientNames = new Set(client)
  return [...new Set(course.filter((name) => clientNames.has(name)))].sort()
}

export async function scanComponentSources(root) {
  const findings = []
  for (const component of ['adapters', 'client', 'course']) {
    await visit(path.join(root, component), root, findings, component)
  }
  return findings.sort((left, right) => {
    const fileOrder = left.file.localeCompare(right.file)
    return fileOrder === 0 ? left.line - right.line : fileOrder
  })
}

async function visit(directory, root, findings, component) {
  let entries
  try {
    entries = await readdir(directory, { withFileTypes: true })
  } catch (error) {
    if (error?.code === 'ENOENT') {
      return
    }
    throw error
  }

  for (const entry of entries) {
    const entryPath = path.join(directory, entry.name)
    if (entry.isDirectory()) {
      if (!excludedDirectories.has(entry.name)) {
        await visit(entryPath, root, findings, component)
      }
      continue
    }
    if (!sourceExtensions.has(path.extname(entry.name))) {
      continue
    }
    const source = await readFile(entryPath, 'utf8')
    for (const finding of findForbiddenReferences(source, component)) {
      findings.push({
        file: path.relative(root, entryPath),
        ...finding,
      })
    }
  }
}

async function main() {
  const root = process.cwd()
  const findings = await scanComponentSources(root)
  const duplicates = await findDuplicateFixtureNames(root)
  if (findings.length === 0 && duplicates.length === 0) {
    console.log('Component test isolation check passed.')
    return
  }
  for (const finding of findings) {
    console.error(
      `${finding.file}:${finding.line}: forbidden component reference ${finding.reference}`,
    )
  }
  for (const name of duplicates) {
    console.error(
      `duplicate test fixture: course/test-data/${name} and client/test-data/${name}`,
    )
  }
  process.exitCode = 1
}

if (isMain(import.meta.url)) {
  await main()
}
