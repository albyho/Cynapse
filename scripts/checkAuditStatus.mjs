import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import { isMain } from './isMain.mjs'

const HEADING = /^#{3,4}\s+((?:VULN|AUD)-\d+)\b(.*)$/
const STATUS_TOKEN = /\b(open|fixed|deferred|needs-decision|closed-by-decision)\b/g
const STATUS_LINE = /^- \*\*修复状态：\*\* (.*)$/m
const APPENDIX_ROW = /^\|\s*((?:VULN|AUD)-\d+)\s*\|.*\|\s*`([a-z-]+)`\s*\|\s*$/

function statusTokens(text) {
  return [...new Set(text.match(STATUS_TOKEN) ?? [])].sort()
}

function headingStatusTokens(rest) {
  const tail = /[（(]([^（）()]*)[）)]\s*$/.exec(rest)
  return statusTokens(tail ? tail[1] : '')
}

/**
 * Check that an audit report's section heading, 修复状态 line, and appendix
 * row carry the same status. Headings may list one status (VULN reports) or
 * several (AUD reports); files without supported headings are ignored.
 */
export function auditStatusMismatches(markdown, file = 'audit.md') {
  const lines = markdown.split(/\r?\n/)
  const headings = []
  for (let index = 0; index < lines.length; index += 1) {
    const match = HEADING.exec(lines[index])
    if (!match) {
      continue
    }
    const statuses = headingStatusTokens(match[2])
    if (statuses.length > 0) {
      headings.push({ id: match[1], statuses, index })
    }
  }
  if (headings.length === 0) {
    return []
  }

  const appendix = new Map()
  for (const line of lines) {
    const match = APPENDIX_ROW.exec(line)
    if (match) {
      appendix.set(match[1], match[2])
    }
  }

  const mismatches = []
  for (let index = 0; index < headings.length; index += 1) {
    const heading = headings[index]
    if (appendix.size > 0 && !appendix.has(heading.id)) {
      mismatches.push(`${file}: ${heading.id} has no appendix row`)
      continue
    }

    let end = lines.length
    for (let next = heading.index + 1; next < lines.length; next += 1) {
      if (/^#{1,6}\s/.test(lines[next])) {
        end = next
        break
      }
    }
    const section = lines.slice(heading.index, end).join('\n')
    const statusLine = STATUS_LINE.exec(section)
    if (!statusLine) {
      mismatches.push(`${file}: ${heading.id} has no 修复状态 line`)
      continue
    }

    const actual = statusTokens(statusLine[1])
    if (actual.length === 0) {
      mismatches.push(`${file}: ${heading.id} has no status token in 修复状态`)
      continue
    }
    if (actual.join(',') !== heading.statuses.join(',')) {
      mismatches.push(
        `${file}: ${heading.id} heading says ${heading.statuses.join(', ')}, 修复状态 says ${actual.join(', ')}`,
      )
    }
    const expected = appendix.get(heading.id)
    if (expected) {
      const expectedTokens = statusTokens(expected)
      if (expectedTokens.join(',') !== heading.statuses.join(',')) {
        mismatches.push(
          `${file}: ${heading.id} heading says ${heading.statuses.join(', ')}, appendix says ${expectedTokens.join(', ')}`,
        )
      }
    }
  }
  return mismatches
}

function main() {
  const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
  const directory = join(root, 'docs', 'audits')
  // The public checkout mirrors source only; there is nothing to check there.
  if (!existsSync(directory)) {
    console.log('no docs/audits in this checkout; skipping audit status check.')
    return
  }
  const files = readdirSync(directory).filter(
    (name) => name.endsWith('.md') && name.includes('audit'),
  )
  const mismatches = files.flatMap((name) =>
    auditStatusMismatches(readFileSync(join(directory, name), 'utf8'), name),
  )

  if (mismatches.length > 0) {
    console.error('Audit status mismatch:')
    for (const mismatch of mismatches) {
      console.error(`- ${mismatch}`)
    }
    process.exit(1)
  }
  console.log('audit statuses consistent.')
}

if (isMain(import.meta.url)) {
  main()
}
