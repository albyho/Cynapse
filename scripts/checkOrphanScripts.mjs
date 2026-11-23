import { readdirSync, readFileSync } from 'node:fs'
import path from 'node:path'
import { isMain } from './isMain.mjs'

const SCRIPT_EXTENSIONS = new Set(['.mjs', '.ps1'])

export function listScripts(root) {
  return readdirSync(path.join(root, 'scripts'), { withFileTypes: true })
    .filter(
      (entry) =>
        entry.isFile() && SCRIPT_EXTENSIONS.has(path.extname(entry.name)),
    )
    .map((entry) => entry.name)
    .sort()
}

function workflowTexts(root) {
  const directory = path.join(root, '.github/workflows')
  try {
    return readdirSync(directory, { withFileTypes: true })
      .filter(
        (entry) =>
          entry.isFile() &&
          (entry.name.endsWith('.yml') || entry.name.endsWith('.yaml')),
      )
      .map((entry) => readFileSync(path.join(directory, entry.name), 'utf8'))
  } catch (error) {
    if (error?.code === 'ENOENT') {
      return []
    }
    throw error
  }
}

export function findOrphanScripts(root) {
  const names = listScripts(root)
  const packageJson = readFileSync(path.join(root, 'package.json'), 'utf8')
  const workflows = workflowTexts(root)
  const scripts = names.map((name) =>
    readFileSync(path.join(root, 'scripts', name), 'utf8'),
  )

  return names.filter((name) => {
    const references = [packageJson, ...workflows]
    for (const [index, other] of names.entries()) {
      if (other !== name) {
        references.push(scripts[index])
      }
    }
    return !references.some((text) => text.includes(name))
  })
}

function main() {
  const orphans = findOrphanScripts(process.cwd())
  if (orphans.length === 0) {
    console.log('all repo scripts are referenced')
    return
  }

  console.error('Unreferenced scripts:')
  for (const name of orphans) {
    console.error(`  scripts/${name}`)
  }
  process.exitCode = 1
}

if (isMain(import.meta.url)) {
  main()
}
