import { type Position } from './applyModel'
import { ApplyError } from './applyModel'

export function matchTextPrefix(
  actual: string,
  expected: string,
): { matched: number; consumed: number } {
  let expectedIndex = 0
  let actualIndex = 0

  while (
    expectedIndex < expected.length &&
    actualIndex < actual.length
  ) {
    if (
      expected[expectedIndex] === '\r' ||
      expected[expectedIndex] === '\n'
    ) {
      const expectedStep =
        expected.startsWith('\r\n', expectedIndex) ? 2 : 1
      if (actual.startsWith('\r\n', actualIndex)) {
        actualIndex += 2
      } else if (
        actual[actualIndex] === '\r' ||
        actual[actualIndex] === '\n'
      ) {
        actualIndex += 1
      } else {
        break
      }
      expectedIndex += expectedStep
      continue
    }

    if (actual[actualIndex] !== expected[expectedIndex]) {
      break
    }
    expectedIndex += 1
    actualIndex += 1
  }

  return {
    matched: expectedIndex,
    consumed: actualIndex,
  }
}

export function normalizeLineEndings(text: string): string {
  return text.replace(/\r\n?/g, '\n')
}

export function advancePosition(position: Position, text: string): Position {
  const lines = text.split('\n')
  if (lines.length === 1) {
    return {
      line: position.line,
      character: position.character + text.length,
    }
  }

  return {
    line: position.line + lines.length - 1,
    character: lines[lines.length - 1].length,
  }
}
export interface TextMatch {
  start: number
  end: number
}
export function lineStartOffset(lines: string[], line: number): number {
  let offset = 0
  for (let index = 0; index < line - 1; index += 1) {
    offset += lines[index].length + 1
  }
  return offset
}

export function ambiguousMatch(text: string, count: number): ApplyError {
  return new ApplyError(
    'ambiguous_code_match',
    `expected exactly one match for ${text}, found ${count}`,
  )
}

export function missingMatch(text: string): ApplyError {
  return new ApplyError('code_match_missing', `could not find ${text}`)
}

export function findTextMatches(actual: string, expected: string): TextMatch[] {
  const normalized = normalizeWithOffsets(actual)
  const target = normalizeLineEndings(expected)
  if (target.length === 0) {
    return []
  }

  const matches: TextMatch[] = []
  let index = normalized.text.indexOf(target)
  while (index >= 0) {
    matches.push({
      start: normalized.offsets[index],
      end: normalized.offsets[index + target.length],
    })
    index = normalized.text.indexOf(target, index + target.length)
  }
  return matches
}

export function normalizeWithOffsets(actual: string): {
  text: string
  offsets: number[]
} {
  let text = ''
  const offsets: number[] = []
  for (let index = 0; index < actual.length; index += 1) {
    const character = actual[index]
    offsets.push(index)
    if (character === '\r') {
      text += '\n'
      if (actual[index + 1] === '\n') {
        index += 1
      }
    } else {
      text += character
    }
  }
  offsets.push(actual.length)
  return { text, offsets }
}

export function positionAtOffset(text: string, offset: number): Position {
  const before = text.slice(0, offset)
  const lines = before.split('\n')
  return {
    line: lines.length - 1,
    character: lines[lines.length - 1].replace(/\r$/, '').length,
  }
}
export function suffixPrefixLength(actual: string, expected: string): number {
  const expectedCharacters = Array.from(expected)
  if (expectedCharacters.length === 0) {
    return 0
  }

  // KMP prefix table; scan actual once and keep the longest expected prefix
  // that ends at the current character.
  const prefix = new Array<number>(expectedCharacters.length).fill(0)
  for (let index = 1; index < expectedCharacters.length; index += 1) {
    let length = prefix[index - 1]!
    while (
      length > 0 &&
      expectedCharacters[index] !== expectedCharacters[length]
    ) {
      length = prefix[length - 1]!
    }
    if (expectedCharacters[index] === expectedCharacters[length]) {
      length += 1
    }
    prefix[index] = length
  }

  const actualCharacters = Array.from(actual)
  let matched = 0
  for (let index = 0; index < actualCharacters.length; index += 1) {
    const character = actualCharacters[index]!
    while (
      matched > 0 &&
      character !== expectedCharacters[matched]
    ) {
      matched = prefix[matched - 1]!
    }
    if (character === expectedCharacters[matched]) {
      matched += 1
    }
    if (
      matched === expectedCharacters.length &&
      index + 1 < actualCharacters.length
    ) {
      matched = prefix[matched - 1]!
    }
  }

  return expectedCharacters
    .slice(0, matched)
    .reduce((total, character) => total + character.length, 0)
}

export function characterBoundaries(text: string): number[] {
  const boundaries = [0]
  let offset = 0
  while (offset < text.length) {
    if (text.startsWith('\r\n', offset)) {
      offset += 2
    } else {
      const character = String.fromCodePoint(text.codePointAt(offset)!)
      offset += character.length
    }
    boundaries.push(offset)
  }
  return boundaries
}
