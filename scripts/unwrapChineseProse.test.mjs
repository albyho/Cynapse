import { strict as assert } from 'node:assert'
import test from 'node:test'

import { joinLine, unwrapText } from './unwrapChineseProse.mjs'

test('joins a wrapped Chinese paragraph without a space', () => {
  const { text, count } = unwrapText('中文第一行\n中文第二行。\n')
  assert.equal(text, '中文第一行中文第二行。\n')
  assert.equal(count, 1)
})

test('keeps the space that a soft break stands for between scripts', () => {
  assert.equal(joinLine('启动 Client', '然后连接'), '启动 Client 然后连接')
  assert.equal(joinLine('客户端写入 `runtime.json`；', '字段由 schema 定义。'), '客户端写入 `runtime.json`；字段由 schema 定义。')
})

test('never glues a full-width colon onto the previous line', () => {
  assert.equal(joinLine('**Course Tree**', '：节点只有一种类型'), '**Course Tree**：节点只有一种类型')
})

test('leaves English prose wrapped', () => {
  const source = 'An English paragraph that is\nwrapped by hand.\n'
  const { text, count } = unwrapText(source)
  assert.equal(text, source)
  assert.equal(count, 0)
})

test('leaves fenced code alone', () => {
  const source = '```text\n中文一行\n中文二行\n```\n'
  const { text, count } = unwrapText(source)
  assert.equal(text, source)
  assert.equal(count, 0)
})

test('leaves tables and headings alone', () => {
  const source = '| 路径 | 内容 |\n| --- | --- |\n| `client/` | 客户端 |\n\n## 标题\n正文。\n'
  const { text, count } = unwrapText(source)
  assert.equal(text, source)
  assert.equal(count, 0)
})

test('keeps the blockquote marker and merges its wrapped text', () => {
  assert.equal(joinLine('> 这是一段引用，', '继续引用。'), '> 这是一段引用，继续引用。')
})

test('keeps an explicit line break', () => {
  const source = '第一行中文内容。  \n第二行中文内容。\n'
  const { text } = unwrapText(source)
  assert.equal(text, source)
})

test('merges a list item with its continuation line', () => {
  const { text } = unwrapText('- 列表项第一行，\n  第二行。\n')
  assert.equal(text, '- 列表项第一行，第二行。\n')
})

test('keeps link definitions and indented code blocks intact', () => {
  const definition = '[docs]: https://example.com\n中文说明写在这里。\n'
  assert.equal(unwrapText(definition).text, definition)
  const indented = '普通一行\n    code_in_indented_block\n'
  assert.equal(unwrapText(indented).text, indented)
})

test('leaves YAML front matter alone', () => {
  const source = '---\nname: 测试技能\ndescription: 中文说明写在这里，\n  下一行继续写。\n---\n\n正文中文内容。\n'
  assert.equal(unwrapText(source).text, source)
})

test('leaves English paragraphs that quote a Chinese label alone', () => {
  const source =
    'A Step with `requires_human: true` waits at the Step boundary instead of\nadvancing automatically. It then shows “下一步” and stops there.\n'
  const { text, count } = unwrapText(source)
  assert.equal(text, source)
  assert.equal(count, 0)
})
