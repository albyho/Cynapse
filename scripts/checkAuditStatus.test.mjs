import assert from 'node:assert/strict'
import test from 'node:test'

import { auditStatusMismatches } from './checkAuditStatus.mjs'

function report({ heading, status, appendix, extraRow = '' }) {
  return `#### VULN-001 example (${heading})
- **修复状态：** \`${status}\`（2026-11-20）。

| ID | Level | Type | Path | Status |
| --- | --- | --- | --- | --- |
| VULN-001 | High | Type | \`path\` | \`${appendix}\` |
${extraRow}`
}

test('matching heading, status, and appendix pass', () => {
  assert.deepEqual(auditStatusMismatches(report({ heading: 'fixed', status: 'fixed', appendix: 'fixed' })), [])
})

test('heading and 修复状态 disagree', () => {
  const mismatches = auditStatusMismatches(
    report({ heading: 'fixed', status: 'open', appendix: 'fixed' }),
  )
  assert.match(mismatches.join('\n'), /heading says fixed, 修复状态 says open/)
})

test('修复状态 and appendix disagree', () => {
  const mismatches = auditStatusMismatches(
    report({ heading: 'fixed', status: 'fixed', appendix: 'open' }),
  )
  assert.match(mismatches.join('\n'), /heading says fixed, appendix says open/)
})

test('missing 修复状态 line is reported', () => {
  const markdown = `#### VULN-001 example (fixed)

| ID | Level | Type | Path | Status |
| --- | --- | --- | --- | --- |
| VULN-001 | High | Type | \`path\` | \`fixed\` |
`
  assert.match(auditStatusMismatches(markdown).join('\n'), /no 修复状态 line/)
})

test('missing appendix row is reported when the table exists', () => {
  const markdown = report({
    heading: 'fixed',
    status: 'fixed',
    appendix: 'fixed',
    extraRow: '| VULN-002 | High | Type | `path` | `open` |\n',
  }).replace('| VULN-001 | High | Type | `path` | `fixed` |\n', '')
  assert.match(auditStatusMismatches(markdown).join('\n'), /no appendix row/)
})

test('reports without audit headings are ignored', () => {
  assert.deepEqual(auditStatusMismatches('# Notes\n\nNo findings here.\n'), [])
})

test('AUD headings may list several statuses', () => {
  const markdown = `### AUD-29 example（窗口按钮 fixed，进度标签 closed-by-decision）
- **修复状态：** 窗口按钮 \`fixed\`（2026-11-19）；进度标签 \`closed-by-decision\`。
`
  assert.deepEqual(auditStatusMismatches(markdown), [])
})

test('AUD heading statuses must match the 修复状态 line', () => {
  const markdown = `### AUD-29 example（窗口按钮 fixed，进度标签 closed-by-decision）
- **修复状态：** 窗口按钮 \`open\`；进度标签 \`closed-by-decision\`。
`
  assert.match(
    auditStatusMismatches(markdown).join('\n'),
    /heading says closed-by-decision, fixed, 修复状态 says closed-by-decision, open/,
  )
})

test('AUD headings must include the 修复状态 line', () => {
  assert.match(
    auditStatusMismatches('### AUD-09 example（fixed）\n\nDetails.\n').join('\n'),
    /no 修复状态 line/,
  )
})

test('AUD status lines must contain a canonical token', () => {
  assert.match(
    auditStatusMismatches(
      '### AUD-02 example（fixed）\n- **修复状态：** （修复）2026-11-18。\n',
    ).join('\n'),
    /no status token/,
  )
})
