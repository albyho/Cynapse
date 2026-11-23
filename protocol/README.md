# Cynapse Protocol

`schema.json` is the v3 wire contract for the Client–Adapter envelope. It is not loaded at runtime; Client/core and the Adapter test suites both validate golden messages against the schema.

Protocol samples are split by ownership:

- Reusable samples shared by multiple Protocol implementations live in
  `protocol/test-data/`.
- Representative schema-invalid messages shared by multiple implementations
  live in `protocol/invalid-test-data/`.
- Component-specific test data lives with that component and is not shared.
- Course Content Packages do not contain special `protocol/` directories. Tests
  needing expected messages for specific content own those expectations next to
  the tests.

`courses/` is a removable Content Package collection. Explicit content-validation
and debugging commands may target it, but removing the directory must not break
Client or Adapter tests.

## Action payloads

`action` messages carry one primitive action:

- Course-local `Wait` is not a Protocol action. The Client owns its timer and
  never sends it to an Adapter.
- `Present` is a side-effect-free presentation beat. Narration and captions
  provide the teaching content; Adapters acknowledge it without touching the
  workspace.
- `OpenFile` opens or activates an existing Leaf Workspace file and never
  creates a missing file or directory.
- `InsertText` inserts authored text at a zero-width unified Anchor in an
  already-open file. The Anchor is `{ line?, at?, placement?, eof? }`: `at` is
  a zero-based Unicode-scalar column or `{ text, index? }`; `line` is 1-based
  and required for line/column anchors; `placement` is before/after; `eof`
  marks the file end. Insertion is idempotent against an already-present text
  prefix and saves before ACK. A text anchor scopes to `line` when present;
  omitted `index` means the first match. An omitted, null, or zero `speed_ms`
  inserts instantly; a positive `speed_ms` types the text one visual step at a
  time.
- `DeleteText` removes an Edit Range or deletes a specified number of Unicode
  scalar values before or after a Cursor Deletion target. Cursor Deletion also
  requires the non-empty `expected_text` it removes, so an interrupted Step can
  prove whether the deletion already completed. Action-level `direction`
  selects Backspace or forward-Delete animation for ranges and character
  deletion; it defaults to Backspace for ranges and is required for cursor
  targets. An omitted, null, or zero `speed_ms` deletes instantly; a positive
  `speed_ms` animates the deletion.
- `ReplaceText` replaces a non-empty Edit Range with non-empty authored
  `text`. A missing target is an error, even if the requested replacement text
  already appears elsewhere.
- `CreateDirectory` creates a directory. `create_parents` defaults to `true`.
- `CreateFile` creates an empty file without overwriting existing content. `create_parents` defaults to `true`.
- `DeleteFile` deletes a file. `if_missing` defaults to `error`; set it to `ignore` for deterministic replay.
- `DeleteDirectory` deletes an empty directory by default. `recursive: true` requires Adapter-side user confirmation. `if_missing` defaults to `error`.
- `TerminalEnsure` creates or reuses a dedicated Terminal Session and reveals it without taking keyboard focus. `session_key` is the terminal identity: repeated keys reuse one terminal across Steps, Actions, and working directories; a new key explicitly requests another terminal.
- `TerminalCommand` executes one command in a Terminal Session and requires the shell's real exit code. It reuses the terminal identified by `session_key` by default; set `new_terminal: true` to replace that terminal before executing the command.

All filesystem paths are Course Workspace-relative, normalized, and must remain inside the Course Workspace. `action` messages also carry the target `unit_path`; write targets and text edits must resolve inside `<Course Workspace>/<unit_path>/`. Read/open paths may use `..` inside the Course Workspace. Creation and deletion are idempotent: creating an existing target of the same type is a no-op, while deletion with `if_missing: "ignore"` succeeds when the target is already absent.

Delete and Replace ranges use one kindless Edit Range: `start`, `end`, and a
non-empty `expected_text`. Both ends are the same unified Anchor described for
`InsertText`; there is no `include_end_line_break` switch. Insert and range
start default to `before`; a range end defaults to `after` for line-level and
text anchors and to `before` for column anchors. A range end at a line end
always includes the following CRLF/LF and the expected text must contain it;
the start never includes a preceding line break. Text anchors may mix with
line/column anchors; omitted text `index` means 0.

Both anchors resolve to normalized document offsets; start must be strictly
before end. The complete normalized selection must equal `expected_text`.
Cursor Deletion uses `target.cursor` with line, column, and a positive count,
plus the non-empty `expected_text` that the cursor operation removes, instead
of Edit Range fields. The Rust loader and both Adapters reject old
negotiated-field shapes, unknown/null Anchor fields, `eof` mixed with other
fields, empty text, and a same-text range that resolves to an empty interval.

`ReplaceText` replaces instantly when `speed_ms` is omitted, null, or 0. With a positive `speed_ms` typing interval, `edit_mode` selects one of two editing presentations:

- `overwrite_selection` selects the source range and replaces it with the first typed chunk before continuing to type. This models selecting text and directly entering its replacement.
- `delete_then_type` deletes the complete source selection in reverse chunks, then types the complete replacement. The Adapter performs both stages inside one Action so validation, acknowledgement, cancellation, and retry remain atomic from the Client's perspective.

Cancelling any editing Action restores its complete pre-Action document text before the action returns, so retrying cannot leave a half-edited fragment.

Cancellation is checked within the active Action, including while it is saving. Already completed Actions are not rolled back; an Action that has not started is not executed.

Terminal commands accept exit code `0` by default. A course may list other accepted codes for intentional failure demonstrations. Every string in `expected_output` must occur after ANSI control sequences are removed and line endings are normalized. Output is bounded to a 128 KiB tail with an explicit truncation marker; if only the tail is available and an expected string is absent, the Adapter returns `terminal_output_truncated` instead of `terminal_output_mismatch`. If VS Code Shell Integration is unavailable, the Adapter returns `shell_integration_unavailable` instead of treating `sendText` submission as success.

Terminal acknowledgements may include `exit_code`, normalized `output`, `output_truncated`, and `completion_marker_found`. The Client uses these values for environment checks and playback diagnostics. After a successful environment check, the Client closes the check terminal through terminal-only cleanup and treats cleanup failure as an environment-check failure. A failed check leaves the terminal open for diagnosis.

`workspace_reset` with a non-empty `paths` list closes matching Cynapse-managed terminals first and waits for them to release the workspace, then closes matching tabs before files are removed. An optional `exclude_paths` list keeps matching descendant workspaces open; a parent Unit reset uses it to preserve child Unit terminals and tabs. `close_tabs: false` performs terminal cleanup only and preserves editor tabs; this is the Stop path. An empty `paths` list is valid and means that no workspace files need removal; the Adapter still closes all Cynapse-managed terminals for the current Attempt.

`cancel` may include `reason: "user_stop" | "client_exit" | "course_switch"`. The Adapter aborts the active Action on cancellation and on WebSocket disconnect. A cancel targets both the active request and queued one-off requests by `target_id`; a queued request acknowledges `cancelled` when it starts instead of executing. Terminal execution maps interruption to Ctrl+C and retains the bounded partial output. A timeout is acknowledged as `cancelled` with `code: "terminal_timeout"`; the Client exposes it as a retryable failure rather than advancing playback.

Timeline steps may contain `actions` instead of a single `action`. The Client flattens them for playback so each primitive keeps its own action id, acknowledgement, cancellation boundary, idempotency key, and error result.
