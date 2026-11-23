import { normalizeTerminalOutput } from './terminal'

export type TerminalShell = 'powershell' | 'posix'

export const BEGIN_MARKER = '__CYNAPSE_BEGIN__'
export const EXIT_MARKER = '__CYNAPSE_EXIT__'

// The marker line is only complete once its line terminator arrives: output is
// delivered in chunks, and `__CYNAPSE_EXIT__:1` can be the first half of `127`.
const EXIT_LINE_PATTERN = /__CYNAPSE_EXIT__:(-?\d+)(?:\r\n|\r|\n)/
const EXIT_PATTERN = new RegExp(EXIT_LINE_PATTERN.source, 'g')
const BEGIN_LINE_PATTERN = /(?:^|\n)__CYNAPSE_BEGIN__\n/g

/** True once a complete exit-marker line has arrived, not on a partial chunk. */
export function exitMarkerComplete(raw: string): boolean {
  return EXIT_LINE_PATTERN.test(raw)
}

export interface ParsedTerminalCommandOutput {
  output: string
  exitCode: number | undefined
  markerFound: boolean
}

/**
 * A Jupyter terminal is a plain shell, so the command and its exit status are
 * delimited by markers. The begin marker hides the shell's echo of the wrapper;
 * the exit marker cannot collide with that echo because it copies `$cynapseCode`
 * or `%s`, never a number.
 */
export function wrapTerminalCommand(
  shell: TerminalShell,
  command: string,
): string {
  if (shell === 'powershell') {
    return [
      `Write-Output '${BEGIN_MARKER}'`,
      '$global:LASTEXITCODE = $null',
      `& { ${command.trim()} }`,
      '$cynapseOk = $?',
      '$cynapseCode = if ($cynapseOk) { 0 } elseif ($null -ne $LASTEXITCODE) { $LASTEXITCODE } else { 1 }',
      `Write-Output "\`n${EXIT_MARKER}:$cynapseCode"`,
    ].join('; ')
  }

  const normalized = command.trim().replace(/[;\s]+$/, '')
  return [
    `printf '\\n${BEGIN_MARKER}\\n'`,
    normalized,
    'cynapse_code=$?',
    `printf '\\n${EXIT_MARKER}:%s\\n' "$cynapse_code"`,
  ].join('; ')
}

export function parseTerminalCommandOutput(
  raw: string,
): ParsedTerminalCommandOutput {
  const text = normalizeTerminalOutput(raw)
  const exit = lastExitMatch(text)
  if (!exit) {
    return {
      output: textAfterBeginMarker(text),
      exitCode: undefined,
      markerFound: false,
    }
  }

  const beforeExit = text.slice(0, exit.index)
  return {
    output: stripOneTrailingNewline(textAfterBeginMarker(beforeExit)),
    exitCode: Number(exit.value),
    markerFound: true,
  }
}

function lastExitMatch(
  text: string,
): { index: number; value: string } | undefined {
  let match: RegExpExecArray | undefined
  for (const candidate of text.matchAll(EXIT_PATTERN)) {
    match = candidate
  }
  return match === undefined || match.index === undefined
    ? undefined
    : { index: match.index, value: match[1] }
}

function textAfterBeginMarker(text: string): string {
  let index = -1
  for (const match of text.matchAll(BEGIN_LINE_PATTERN)) {
    index = match.index
  }
  if (index < 0) {
    return text
  }
  const marker = text.slice(index).match(/^\n?__CYNAPSE_BEGIN__\n/)
  return text.slice(index + (marker?.[0].length ?? 0))
}

function stripOneTrailingNewline(value: string): string {
  return value.endsWith('\n') ? value.slice(0, -1) : value
}
