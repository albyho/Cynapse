import { execFileSync } from 'node:child_process'

execFileSync('git', ['config', 'core.hooksPath', '.githooks'], {
  stdio: 'inherit',
})
console.log('installed git hooks from .githooks')
