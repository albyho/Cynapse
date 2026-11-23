#!/usr/bin/env node
// 中文段落不硬换行：把折成多行的中文散文合并回一行。
//
// 英文按列宽硬换行是有意义的（单词边界清晰，diff 只看改动的那一行）；中文
// 没有单词边界，按列宽折断只会在源码里制造假的换行，改一个字就要重排整段。
// 所以这里按「段」判断：一段里汉字占字母数的 1/4 以上才算中文散文，英文段落
// 即使引用了「下一步」这类中文界面上限，也不会被误合并。
//
// `--check` 只报告需要合并的文件（退出码 1），`--write` 就地合并，`--root`
// 指向别的检出。`docs/audits`、`docs/research`、`courses` 默认跳过：前两者是
// 历史记录，后者是与上游对齐的课程内容。

import { readFileSync, readdirSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { isMain } from './isMain.mjs'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')

const SKIP_DIRS = new Set([
  '.git',
  '.workbuddy',
  '.codebuddy',
  'node_modules',
  'target',
  'dist',
  '.vscode-test',
])

/** 这些路径下的 Markdown 不参与合并：历史记录与课程内容。 */
const SKIP_PATHS = ['docs/audits', 'docs/research', 'courses', 'adapters/vscode/.vscode-test']

const HAN = /[\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff]/
const FULLWIDTH = /[\u3000-\u303f\uff00-\uffef]/

/** 一段里汉字占比达到这个比例，才当作中文散文。 */
const HAN_RATIO = 0.25

/** 自己成块、绝不与上一行合并的行。 */
const OWN_BLOCK = /^\s*(#{1,6}\s|---+\s*$|===+\s*$|\||```|~~~|<| {4,}\S)/

/** 开启新块的行：列表项、引用、普通段落都从这里重新开始。 */
const LIST_ITEM = /^\s*([-*+]\s|\d+[.)]\s)/

function isFence(line) {
  return /^\s*(```|~~~)/.test(line)
}

function counts(text) {
  // 行内代码是标识符而不是散文，先剥掉再数，否则满是 `verify:client` 的中文段落会被判成英文。
  const prose = text.replace(/`[^`]*`/g, '')
  const han = prose.match(/[\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff]/g) ?? []
  const latin = prose.match(/[A-Za-z]/g) ?? []
  return { han: han.length, latin: latin.length }
}

/** 一段文本是否以中文为主。 */
export function isChineseProse(text) {
  const { han, latin } = counts(text)
  if (han === 0) {
    return false
  }
  return han / (han + latin) >= HAN_RATIO
}

/**
 * 把 `current` 接到 `previous` 后面。全角标点两侧不留空格；汉字与拉丁文相邻
 * 时按中英混排习惯补一个空格。
 */
export function joinLine(previous, current) {
  const body = previous.replace(/\s+$/, '')
  const tail = current.replace(/^\s*/, '')
  const last = body.slice(-1)
  const first = tail.slice(0, 1)
  const noSpace = FULLWIDTH.test(last) || FULLWIDTH.test(first) || (HAN.test(last) && HAN.test(first))
  return noSpace ? `${body}${tail}` : `${body} ${tail}`
}

/** 行尾两个空格或反斜杠是作者显式换行；链接定义自成一块。 */
function canJoin(previous, current) {
  if (/ {2,}$/.test(previous) || /\\$/.test(previous)) {
    return false
  }
  return !/^\s*\[[^\]]+\]:/.test(previous) && !/^\s*\[[^\]]+\]:/.test(current)
}

/** 返回合并后的正文；`count` 是发生的合并次数。 */
export function unwrapText(text) {
  const all = text.split('\n')
  // YAML front matter 逐行原样保留：合并会把 name/description 接成一行。
  let head = []
  let lines = all
  if (all[0]?.trim() === '---') {
    const end = all.findIndex((line, index) => index > 0 && line.trim() === '---')
    if (end !== -1) {
      head = all.slice(0, end + 1)
      lines = all.slice(end + 1)
    }
  }
  const out = []
  let count = 0
  let fence = false
  let start = -1
  let quoted = false

  const flush = () => {
    if (start >= 0 && out.length - start > 1 && isChineseProse(out.slice(start).join('\n'))) {
      const merged = [out[start]]
      for (let index = start + 1; index < out.length; index += 1) {
        if (!canJoin(merged[merged.length - 1], out[index])) {
          merged.push(out[index])
          continue
        }
        merged[merged.length - 1] = joinLine(merged[merged.length - 1], out[index])
        count += 1
      }
      out.splice(start, out.length - start, ...merged)
    }
    start = -1
    quoted = false
  }

  for (const line of lines) {
    if (isFence(line)) {
      flush()
      fence = !fence
      out.push(line)
      continue
    }
    if (fence) {
      out.push(line)
      continue
    }
    if (line.trim() === '') {
      flush()
      out.push(line)
      continue
    }
    if (OWN_BLOCK.test(line)) {
      flush()
      out.push(line)
      continue
    }
    const isQuote = line.startsWith('>')
    const continuesQuote = isQuote && quoted && start >= 0
    if (start < 0 || LIST_ITEM.test(line) || (isQuote && !continuesQuote)) {
      flush()
      start = out.length
      quoted = isQuote
      out.push(line)
      continue
    }
    out.push(line)
  }
  flush()

  return { text: [...head, ...out].join('\n'), count }
}

function skipped(relative) {
  return SKIP_PATHS.some((prefix) => relative === prefix || relative.startsWith(`${prefix}/`))
}

export function markdownFiles(root = ROOT) {
  const found = []
  const walk = (directory) => {
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      const full = path.join(directory, entry.name)
      const relative = path.relative(root, full).split(path.sep).join('/')
      if (entry.isDirectory()) {
        if (SKIP_DIRS.has(entry.name) || skipped(relative)) {
          continue
        }
        walk(full)
      } else if (/\.md$/i.test(entry.name) && !skipped(relative)) {
        found.push(relative)
      }
    }
  }
  walk(root)
  return found.sort()
}

export function plan(root = ROOT) {
  const changes = []
  for (const relative of markdownFiles(root)) {
    const before = readFileSync(path.join(root, relative), 'utf8')
    const { text, count } = unwrapText(before)
    if (count > 0) {
      changes.push({ relative, count, text })
    }
  }
  return changes
}

/** 默认只检查；`--write` 就地合并，`--root` 指向别的检出（例如课程仓库）。 */
export function parseArgs(argv) {
  const root = argv.indexOf('--root')
  return {
    write: argv.includes('--write'),
    root: root === -1 ? ROOT : path.resolve(argv[root + 1]),
  }
}

function main() {
  const options = parseArgs(process.argv.slice(2))
  const changes = plan(options.root)
  if (changes.length === 0) {
    console.log('中文散文没有硬换行')
    return
  }
  const total = changes.reduce((sum, change) => sum + change.count, 0)
  if (!options.write) {
    for (const change of changes) {
      console.log(`${change.relative}: ${change.count}`)
    }
    console.error(`${changes.length} 个文件、${total} 处需要合并；加 --write 就地合并`)
    process.exitCode = 1
    return
  }
  for (const change of changes) {
    writeFileSync(path.join(options.root, change.relative), change.text)
    console.log(`${change.relative}: ${change.count}`)
  }
  console.log(`合并 ${changes.length} 个文件、${total} 处`)
}

if (isMain(import.meta.url)) {
  main()
}
