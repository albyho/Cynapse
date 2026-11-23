export interface TypewriterChunk {
  text: string
  /** One unit is one visible keystroke; indentation groups count as one. */
  units: number
}

/** Split source into visual typewriter steps without changing the bytes. */
export function typewriterTokens(source: string, startOfLine = true): string[] {
  const characters = Array.from(source)
  const tokens: string[] = []
  let lineStart = startOfLine
  let index = 0

  while (index < characters.length) {
    if (lineStart && characters[index] === ' ') {
      let spaces = 0
      while (characters[index + spaces] === ' ') {
        spaces += 1
      }
      let remaining = spaces
      while (remaining >= 4) {
        tokens.push('    ')
        remaining -= 4
      }
      while (remaining > 0) {
        tokens.push(' ')
        remaining -= 1
      }
      index += spaces
      lineStart = false
      continue
    }

    const character = characters[index]
    tokens.push(character)
    index += 1
    lineStart = character === '\n'
  }

  return tokens
}

export function splitIntoTypewriterChunks(
  source: string,
  speedMs: number,
  startOfLine = true,
): TypewriterChunk[] {
  if (source.length === 0) {
    return []
  }
  if (speedMs === 0) {
    return [
      { text: source, units: typewriterTokens(source, startOfLine).length },
    ]
  }

  const tokens = typewriterTokens(source, startOfLine)
  const chunkSize = Math.max(1, Math.ceil(50 / speedMs))
  const chunks: TypewriterChunk[] = []
  for (let index = 0; index < tokens.length; index += chunkSize) {
    const group = tokens.slice(index, index + chunkSize)
    chunks.push({ text: group.join(''), units: group.length })
  }
  return chunks
}
