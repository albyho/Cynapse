import { execFileSync } from 'node:child_process'

const output = execFileSync('git', ['ls-files', '--eol'], {
  encoding: 'utf8',
})
const offenders = output
  .split(/\r?\n/)
  .filter((line) => /\battr\/text eol=lf\b/.test(line) && /\bi\/(crlf|mixed)\b/.test(line))

if (offenders.length > 0) {
  console.error('Text files must be stored with LF line endings in the index:')
  for (const line of offenders) {
    console.error(`  ${line}`)
  }
  process.exit(1)
}

console.log('text line endings OK')
