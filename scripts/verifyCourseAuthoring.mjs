#!/usr/bin/env node
// Run Python tests owned by present Content Packages.
//
// `courses/` is optional, so discovery must be a no-op when no package tests
// exist. Each package owns its tests under `courses/<package>/tests`.
import { spawnSync } from 'node:child_process'
import { existsSync, readdirSync } from 'node:fs'
import * as path from 'node:path'
import { isMain } from './isMain.mjs'

const repositoryRoot = path.resolve(import.meta.dirname, '..')
const coursesRoot = path.join(repositoryRoot, 'courses')
const pythonRunner = path.join(import.meta.dirname, 'python.mjs')

export function courseTestDirectories(root) {
  if (!existsSync(root)) {
    return []
  }
  return readdirSync(root, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => path.join(root, entry.name, 'tests'))
    .filter((tests) => existsSync(tests))
    .sort()
}

function main() {
  const tests = courseTestDirectories(coursesRoot)
  if (tests.length === 0) {
    console.log('No Course Package authoring tests found; skipping.')
    return
  }

  let failed = false
  for (const testsDirectory of tests) {
    console.log(
      `Course authoring tests: ${path.relative(repositoryRoot, testsDirectory)}`,
    )
    const result = spawnSync(
      process.execPath,
      [
        pythonRunner,
        '-m',
        'unittest',
        'discover',
        '-s',
        testsDirectory,
        '-v',
      ],
      { stdio: 'inherit' },
    )
    if (result.error) {
      console.error(
        `Failed to run ${path.relative(repositoryRoot, testsDirectory)}: ${result.error.message}`,
      )
      failed = true
    } else if (result.status !== 0) {
      failed = true
    }
  }
  if (failed) {
    process.exitCode = 1
  }
}

if (isMain(import.meta.url)) {
  main()
}
