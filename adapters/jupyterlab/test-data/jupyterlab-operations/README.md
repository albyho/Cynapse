# JupyterLab Operations

Self-contained manual acceptance course for the JupyterLab Adapter.

Run it from the repository root:

```powershell
pnpm dev:jupyterlab -- --course adapters/jupyterlab/test-data/jupyterlab-operations
```

The course covers:

- Markdown preview (`INTRO.md`) and close of the preview/document; real courses
  default to `INTRO.ipynb`, so this fixture intentionally keeps the Markdown
  preview path covered
- The JupyterLab file browser follows and selects opened Notebooks and previews
- Notebook open, Cell selection, Cell insertion before/after
- Cell scroll alignment over a long Notebook: `start`, `center`, `end`, `auto`
  (no movement when already visible), and `top-center` for a Cell taller than
  the viewport. It is its own Unit (`Cell scroll alignment`) with its own
  `scroll_align.ipynb`, so it can be started directly without running the other
  operations first
- First-Cell insertion into a zero-Cell Notebook
- Cell move up/down
- Cell duplication after the source Cell
- Typewriter code entry
- Cell execution for text, numeric, rich output, and errors
- Run one Cell, a range from `setup` to the target, and all Cells
- Single-Cell deletion of Cynapse-inserted cells (range operations are next)
- Close the Notebook at the end (saves dirty state, closes the tab, shuts down
  its Kernel)
- Kernel restart
- Terminal command
- Manual Stop against a long-running Cell

The `manual_stop` step is intentionally long-running. Use Stop in the Client
while it is executing, then resume the Step to verify replay.

Cell IDs are stable and start with `cynapse_`. The course only deletes Cells it
inserted. It is not a published course and must not be moved under `courses/`.
