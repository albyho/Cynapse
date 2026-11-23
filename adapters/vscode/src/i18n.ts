type L10nApi = {
  t: (message: string, ...args: unknown[]) => string
}

/**
 * Translate a user-facing string with VS Code's active display language.
 * The Node tests and non-VS Code hosts have no `vscode` module, so fall back
 * to the English source string with `{0}`-style placeholders filled in.
 */
export function t(message: string, ...args: unknown[]): string {
  try {
    const vscode = require('vscode') as { l10n?: L10nApi }
    if (vscode.l10n) {
      return vscode.l10n.t(message, ...args)
    }
  } catch {
    // No VS Code module: use the English fallback.
  }
  return format(message, args)
}

function format(message: string, args: unknown[]): string {
  return message.replace(/\{(\d+)\}/g, (placeholder, index: string) => {
    const value = args[Number(index)]
    return value === undefined ? placeholder : String(value)
  })
}
