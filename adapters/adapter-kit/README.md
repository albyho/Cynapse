# Cynapse Adapter Kit

Shared TypeScript runtime for the current VS Code and JupyterLab Adapters.

This package is an implementation helper, not the cross-language protocol
contract. The language-neutral contract is `protocol/schema.json` plus the
fixtures under `protocol/test-data/` and `protocol/contracts/`. A future native
Adapter should consume that contract instead of depending on this package.

## Modules

- `protocol`: shared Protocol v2 envelope, ACK, Check, terminal, cancel, and
  workspace-reset types.
- `check`: shared Check payload defaults and validation.
- `connection`: transport-neutral hello, queue, ACK dedupe, cancel, and
  reconnect state machine.
- `text`: typewriter tokenization and chunking used by editor and notebook
  hosts.
- `terminal`: terminal output normalization, bounded-output handling, and
  result classification.

Platform hosts remain in `adapters/vscode` and `adapters/jupyterlab`; the kit
does not import either platform API.
