import { spawnSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const adapterRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const venvPython =
  process.platform === 'win32'
    ? path.join('Scripts', 'python.exe')
    : path.join('bin', 'python')

const candidates = [
  process.env.JUPYTERLAB_BUILD_PYTHON,
  path.join(
    adapterRoot,
    'test-data/jupyterlab-operations/workspace/.venv',
    venvPython,
  ),
  path.join(adapterRoot, '../../.venv', venvPython),
  process.env.PYTHON,
  'python',
].filter((candidate) => candidate)

const python = candidates.find((candidate) => {
  if (candidate !== 'python' && !existsSync(candidate)) {
    return false
  }
  return (
    spawnSync(candidate, ['-c', 'import jupyter_builder'], {
      stdio: 'ignore',
    }).status === 0
  )
})

if (!python) {
  console.error(
    [
      'Cannot find the Python package jupyter-builder.',
      'Install it with:',
      '  python -m pip install -r adapters/jupyterlab/requirements-build.txt',
      'or set JUPYTERLAB_BUILD_PYTHON to a Python with that package.',
    ].join('\n'),
  )
  process.exit(1)
}

const result = spawnSync(
  python,
  ['-m', 'jupyter_builder.main', 'build', '.'],
  { cwd: adapterRoot, stdio: 'inherit' },
)
process.exit(result.status ?? 1)
