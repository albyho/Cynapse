# Course test data

These Course Packages are owned by the shared `cynapse-course` contract and are
used by the course crate, Client tests, and Studio. Client-only fixtures remain
in `client/test-data/`.

They are test inputs, not distributable product content. Repository-level
`courses/` may be removed without changing these tests. Protocol-wide golden
messages belong under `protocol/test-data/` instead.

`caption_markup_cases.json` is shared by the `cynapse-course` caption scanner
tests (validation, visible text, and rendering segments all come from
`course/src/caption_markup`) and the two Python narration tools.

## Fixture shapes

Counts are Unit nodes with `steps` and referenced Step files. A challenge count
is the number of Steps with a `challenge` object. Keep this table in sync when a
fixture's manifest changes.

| Fixture | Units | Steps | Challenges | Notes |
| --- | ---: | ---: | ---: | --- |
| `action-before-narration-demo` | 2 | 3 | 0 | Action narration before/during/after |
| `filesystem-actions` | 1 | 3 | 0 | Create/delete file and directory Actions |
| `flexible-tree-demo` | 7 | 7 | 0 | Mixed grouping and Unit nodes |
| `jupyterlab-demo` | 1 | 1 | 0 | JupyterLab Adapter requirement |
| `multi-level-demo` | 12 | 12 | 0 | Nested Units, including parent + child workspaces |
| `multi-step-demo` | 1 | 2 | 0 | Several Steps in one Unit |
| `narration-demo` | 4 | 9 | 0 | Narration phases and audio assets |
| `phase-timeline-demo` | 9 | 11 | 0 | Narration phase timeline |
| `presentation-demo` | 1 | 1 | 0 | Presentation-only course |
| `resources-and-seeds-demo` | 5 | 8 | 0 | Resource aliases and Seed copies |
| `rust-40-lines` | 1 | 1 | 0 | Small Rust baseline |
| `rust-book-zh-cn` | 1 | 9 | 0 | Book-style multi-Step fixture |
| `rust-hello` | 1 | 2 | 0 | Hello-world Rust flow |
| `rust-variables-demo` | 1 | 6 | 0 | Rust variables flow |
| `terminal-recovery-demo` | 2 | 4 | 0 | Terminal interruption/recovery |
