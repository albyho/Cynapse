# Cynapse JupyterLab Adapter

The extension-side package of the Cynapse JupyterLab Adapter. It runs inside
the learner's JupyterLab page, connects to the Client's loopback control
channel, completes the Protocol handshake, and applies the visible Notebook,
Cell, and Kernel Actions defined by the shared Protocol.

Notebook and Kernel operations sit behind the `JupyterLabFacade` seam; terminal
and Check operations sit behind the `TerminalFacade` seam. The Node tests use
fake facades, so they run without a real JupyterLab. [`src/index.ts`](src/index.ts)
is the real prebuilt extension entry; [`src/jupyterLabFacade.ts`](src/jupyterLabFacade.ts)
implements the Notebook, Cell, Kernel, Check, and Reset facades over
`INotebookTracker`, `IDocumentManager`, and `@jupyterlab/services`.

## Build

```powershell
python -m pip install -r requirements-build.txt
pnpm build:jupyterlab
```

`build:extension` resolves Python through `JUPYTERLAB_BUILD_PYTHON`, an existing
local `.venv`, or `python`; it never relies on `jupyter-builder` being on
`PATH`. The resulting wheel is written to `dist/`.

## Connection contract

- The control channel is a WebSocket on the fixed loopback port `37654`
  (`DEFAULT_ADAPTER_PORT`).
- The extension sends `hello` with `role: "adapter"` and an empty token; the
  Client accepts the connection by the JupyterLab browser `Origin`, not a URL
  token.
- The Client answers `hello_ack` with `adapter: "jupyterlab"`.
- A dropped connection retries after one second; a rejected handshake stops
  retrying until the page reloads.
- The Client reuses the running JupyterLab server and its connected browser page
  for subsequent Units. It opens the URL only when the server is new or no
  adapter connection exists, rather than opening a new tab per Unit.
- When a newer browser tab opens JupyterLab and completes the handshake, the
  Client closes the older page's socket with code `4001`
  (`INACTIVE_ADAPTER_CLOSE_CODE`); the older page reports itself inactive and
  stops retrying. Opening more Notebook/Document tabs in one JupyterLab page
  does not replace the connection; the newest handshake wins, not the focused
  tab.
- `Present` is a Client-local no-op Action; the Adapter acknowledges it without
  touching the Notebook.
- Actions run serially. A duplicate Action id re-sends the cached ACK instead of
  executing again; a cancel aborts the active Action; a disconnect aborts it
  too. Cancel retries Kernel interrupts; if the execution is still busy after
  five seconds, the Kernel is restarted before the cancelled ACK so Stop cannot
  leave a runaway execution behind. A successful restart returns `cancelled`
  with `code: "kernel_restarted"` so the Client can warn that Kernel variables
  were cleared. A failed restart returns `status: "error"` with
  `code: "kernel_restart_failed"` instead of a misleading cancellation.

## Action contract

- `JupyterLabCloseDocument`, `JupyterLabInterruptKernel`,
  `JupyterLabOpenNotebook`, `JupyterLabOpenPreview`, `JupyterLabRestartKernel`,
  `JupyterLabCellDelete`, `JupyterLabCellDeleteText`, `JupyterLabCellDuplicate`,
  `JupyterLabCellInsert`, `JupyterLabCellInsertText`, `JupyterLabCellMove`,
  `JupyterLabCellReplaceText`, `JupyterLabCellRun`, and `JupyterLabCellSelect` are
  validated by `parseJupyterLabAction` in [`src/protocol.ts`](src/protocol.ts).
- Cell and Run actions (`JupyterLabCellDelete`, `JupyterLabCellDeleteText`,
  `JupyterLabCellDuplicate`, `JupyterLabCellInsert`,
  `JupyterLabCellInsertText`, `JupyterLabCellMove`,
  `JupyterLabCellReplaceText`, `JupyterLabCellRun`, `JupyterLabCellSelect`)
  ensure the target Notebook is open.
  Kernel and save operations still require the Notebook to be open; they do not
  start one just to restart, interrupt, or save it.
- The Adapter saves after `JupyterLabCellDelete`, `JupyterLabCellDeleteText`,
  `JupyterLabCellInsert`, `JupyterLabCellInsertText`,
  `JupyterLabCellReplaceText`, and `JupyterLabCellRun`.
- `JupyterLabOpenPreview` renders Markdown and does not implement line-level reveal.
  `JupyterLabOpenNotebook` and `JupyterLabOpenPreview` also navigate the JupyterLab file
  browser to the document directory and select the file.
- `JupyterLabCloseDocument` closes either a Notebook or a preview. A dirty Notebook is
  saved before closing, then its Kernel is shut down; a Markdown preview is
  closed without saving. A missing document is already closed.
- `JupyterLabCellInsertText` inserts at a line, position, literal, or
  end-of-file target before ACK. An omitted, null, or zero `speed_ms` writes
  instantly; a positive speed reveals text one visual step at a time, with each
  leading run of 4 spaces counting as one Tab-like step while still writing
  spaces; a shorter run is written one space at a time. Cancellation leaves the
  current partial source for the next Step replay to replace.
- `JupyterLabCellDeleteText` and `JupyterLabCellReplaceText` use the same speed
  rule: omitted, null, or zero deletes/replaces instantly; a positive speed
  animates the edit. `target_hold_ms` shows the selection or cursor before the
  edit starts.
- `JupyterLabCellInsert` requires stable nbformat Cell ids; an existing id updates the
  source instead of creating a duplicate. Omitting both anchors appends; an
  empty Notebook inserts its first Cell at index 0. `before_cell_id` and
  `after_cell_id` are mutually exclusive.
- `JupyterLabCellMove` moves one Cell up or down by `count` positions (default 1) and
  clamps at the first or last Cell.
- `JupyterLabCellDuplicate` copies the source Cell after itself. `new_cell_id`
  defaults to `<cell_id>_copy`; an existing copy is updated instead of
  duplicated.
- `JupyterLabCellDelete` only removes Cells inserted or duplicated by Cynapse and
  treats a missing Cell as already deleted.
- `JupyterLabCellRun` runs one continuous range of code Cells. `from_cell_id` and
  `to_cell_id` are optional; an omitted start means the first Cell and an
  omitted end means the last Cell. A single Cell sets both ids to the same
  value; omitting both runs all Cells. Markdown Cells are skipped. The timeout
  covers the whole range, and an intermediate Cell error returns
  `cell_execution_failed`. It resolves only after the matching Kernel
  `status: idle`, then returns the last executed Cell's `execution_count`, a
  structured `error`, output metadata, and bounded text. Rich output bytes never
  cross the Protocol.
- `JupyterLabCellSelect` selects and focuses a Cell. `scroll` defaults to true;
  `align` picks the viewport alignment (`auto`, `start`, `center`,
  `top-center`, `end`) and defaults to `auto`, which scrolls minimally and uses
  `top-center` for Cells taller than the viewport.
- Typed facade failures: `notebook_missing`, `notebook_not_open`,
  `notebook_close_incomplete`, `cell_id_missing`, `cell_id_duplicate`,
  `cell_range_invalid`, `cell_execution_failed`, `cell_type_mismatch`,
  `cell_not_generated`, `preview_missing`,
  `preview_not_markdown`, `execution_timeout`, `kernel_dead`,
  `kernel_shutdown_incomplete`, `save_conflict`.

## Terminal and Check contract

- `TerminalEnsure` and `TerminalCommand` use one Jupyter Server terminal per
  `session_key`. A missing, closed, or moved terminal is recreated from the
  session key and working directory; `new_terminal` replaces it explicitly.
- The server is launched with the Course Environment `Scripts` directory first
  on `PATH` and `VIRTUAL_ENV` set, so terminals use the course interpreter.
- A shell wrapper marks the command start and reports the real exit code. The
  ACK carries the bounded output tail, `output_truncated`, and `timed_out`; a
  timeout or cancel interrupts the terminal and drops the session.
- A `completion_marker` that already exists acknowledges success without
  running the command again.
- The `check` message reuses the same terminal fields. A dirty Notebook returns
  `workspace_dirty`; with `save_dirty: true` the Adapter saves the Notebook
  through the notebook facade before running the command.

## Reset contract

- `workspace_reset` with `close_tabs: true` closes every open Notebook under
  `paths` (minus `exclude_paths`), shuts down each Kernel, and only then ACKs.
  Descendant Unit Workspaces, `resources/`, and `.venv` stay open.
- `workspace_reset` with `close_tabs: false` is terminal cleanup: it closes
  Cynapse-managed terminals only and never Notebooks or Kernels.
- `notebook_close_incomplete` and `kernel_shutdown_incomplete` are typed ACK
  errors; the Client keeps the workspace files when no ACK arrives.

The Protocol shape lives in the shared [`protocol/schema.json`](../../protocol/schema.json).

The manual acceptance fixture is
[`test-data/jupyterlab-operations`](test-data/jupyterlab-operations/README.md).
Run it with
`pnpm dev:jupyterlab -- --course adapters/jupyterlab/test-data/jupyterlab-operations`.

## Development

```text
pnpm --filter cynapse-jupyterlab-adapter test
pnpm --filter cynapse-jupyterlab-adapter build:extension
pnpm --filter cynapse-jupyterlab-adapter build:wheel
```

From the repository root, `pnpm build:jupyterlab` runs both build steps.

`build:extension` compiles `src/` to `lib/`, then runs `jupyter-builder`. The
builder is a Python tool, so run this from a build environment with
`pip install jupyter-builder`; `build:wheel` needs `hatchling` and writes the
prebuilt wheel to `dist/`. The generated `lib/`, `out/`, `dist/`, and
`cynapse_jupyterlab_extension/labextension/` trees are ignored.

A debug Client built from this checkout uses the matching wheel in `dist/`
automatically; packaged/release builds use `wheels/` beside the executable.
`CYNAPSE_EXTENSION_WHEEL` still overrides both. `pnpm dev:jupyterlab` rebuilds
the wheel when adapter sources are newer than the newest wheel; pass `--build`
to force a rebuild.

The wheel installs its labextension under
`share/jupyter/labextensions/cynapse-jupyterlab-adapter`, matching the
extension package name. Install it into the Course Environment:

```text
python -m pip install dist/cynapse_jupyterlab_extension-0.2.4-py3-none-any.whl
jupyter labextension list
```

The 4.6.6 browser acceptance covered open/select/source/run (stream and error
outputs), a Cell id surviving an inserted Cell, running-cell cancel with the
Kernel restart fallback, Kernel restart, idle interrupt, Environment/Challenge
Check through a Course Environment terminal, missing Notebook, workspace reset
(session gone and Notebook closed), and two browser tabs where the older page
receives the `4001` inactive notification and does not reconnect. The supported
frontend baseline is JupyterLab 4.6.x; JupyterLab 4.5 was not run.

The action tests compile the package and exercise the handler against fake
`JupyterLabFacade` and `TerminalFacade` seams; the connection harness covers
handshake, reconnect, rejection, duplicate resend, serialization, check,
workspace reset, cancel, and disconnect. Terminal tests cover session reuse,
the exit-code wrapper, bounded output, timeout, and cleanup without a real
JupyterLab server.
