import assert from 'node:assert/strict'
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import test from 'node:test'

import { checkVersions, setVersion } from './setVersion.mjs'

function fixture(version) {
  const root = mkdtempSync(path.join(tmpdir(), 'cynapse-version-'))
  mkdirSync(path.join(root, 'adapters/vscode'), { recursive: true })
  mkdirSync(path.join(root, 'adapters/jupyterlab'), { recursive: true })
  writeFileSync(
    path.join(root, 'Cargo.toml'),
    `[workspace]\nmembers = []\n\n[workspace.package]\nversion = "${version}"\n`,
  )
  writeFileSync(
    path.join(root, 'package.json'),
    `{\n  "name": "root",\n  "version": "${version}"\n}\n`,
  )
  writeFileSync(
    path.join(root, 'adapters/vscode/package.json'),
    `{\n  "name": "adapter",\n  "version": "${version}"\n}\n`,
  )
  writeFileSync(
    path.join(root, 'adapters/jupyterlab/package.json'),
    `{\n  "name": "jupyterlab-adapter",\n  "version": "${version}"\n}\n`,
  )
  writeFileSync(
    path.join(root, 'adapters/jupyterlab/pyproject.toml'),
    `[project]\nname = "jupyterlab-adapter"\nversion = "${version}"\n`,
  )
  return root
}

test('bumps every version declaration', () => {
  const root = fixture('0.2.4')
  try {
    setVersion(root, '0.2.5')
    assert.equal(checkVersions(root), '0.2.5')
    assert.match(
      readFileSync(path.join(root, 'Cargo.toml'), 'utf8'),
      /^version = "0\.2\.5"$/m,
    )
    assert.equal(
      JSON.parse(readFileSync(path.join(root, 'package.json'), 'utf8')).version,
      '0.2.5',
    )
    assert.equal(
      JSON.parse(
        readFileSync(path.join(root, 'adapters/vscode/package.json'), 'utf8'),
      ).version,
      '0.2.5',
    )
    assert.equal(
      JSON.parse(
        readFileSync(path.join(root, 'adapters/jupyterlab/package.json'), 'utf8'),
      ).version,
      '0.2.5',
    )
    assert.match(
      readFileSync(
        path.join(root, 'adapters/jupyterlab/pyproject.toml'),
        'utf8',
      ),
      /^version = "0\.2\.5"$/m,
    )
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('rejects mismatched versions', () => {
  const root = fixture('0.2.4')
  try {
    writeFileSync(
      path.join(root, 'package.json'),
      '{\n  "name": "root",\n  "version": "0.2.3"\n}\n',
    )
    assert.throws(() => checkVersions(root), /package\.json: 0\.2\.3/)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('rejects JupyterLab pyproject version drift', () => {
  const root = fixture('0.2.4')
  try {
    writeFileSync(
      path.join(root, 'adapters/jupyterlab/pyproject.toml'),
      '[project]\nname = "jupyterlab-adapter"\nversion = "0.2.3"\n',
    )
    assert.throws(
      () => checkVersions(root),
      /adapters\/jupyterlab\/pyproject\.toml: 0\.2\.3/,
    )
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})

test('rejects invalid versions', () => {
  const root = fixture('0.2.4')
  try {
    assert.throws(() => setVersion(root, 'v0.2.5'), /Invalid version/)
  } finally {
    rmSync(root, { recursive: true, force: true })
  }
})
