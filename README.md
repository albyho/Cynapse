# Cynapse

[![verify](https://github.com/albyho/Cynapse/actions/workflows/verify.yml/badge.svg?branch=main)](https://github.com/albyho/Cynapse/actions/workflows/verify.yml)
[![VS Marketplace Version](https://vsmarketplacebadges.dev/version/tubumu.cynapse-vscode-adapter.svg)](https://marketplace.visualstudio.com/items?itemName=tubumu.cynapse-vscode-adapter)
[![VS Marketplace Downloads](https://vsmarketplacebadges.dev/downloads/tubumu.cynapse-vscode-adapter.svg)](https://marketplace.visualstudio.com/items?itemName=tubumu.cynapse-vscode-adapter)

English | [简体中文](README.zh-CN.md)

Cynapse (Code + Synapse) is a local-first platform for interactive tutorials and
workflows in real software. A Course Package drives an interactive UI and a
recoverable step state machine, and pairs them with one or more App Adapters, so
learners work through the application itself instead of watching a recording or
using a simulator.

Windows is the first supported platform. The Client core, the wire protocol, and
the App Adapters stay platform-neutral, so further platforms can be added
without changing the course contract.

[Introduction video](https://www.youtube.com/watch?v=fk4KJ7OfV-U)

## Download

| Component | Download |
| --- | --- |
| Cynapse Client + JupyterLab Adapter (Windows x64) | [Microsoft Store](https://apps.microsoft.com/detail/9p145r16w33t) · [`cynapse-client-<version>-windows-x64.zip`](https://github.com/albyho/Cynapse/releases/latest) |
| Cynapse VS Code Adapter | [VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=tubumu.cynapse-vscode-adapter) |

The Store listing installs the Client and keeps it updated. Both channels carry
the same build, with the matching JupyterLab Adapter wheel bundled, so the
Adapter needs no separate install. See the [release log](CHANGELOG.md) for what
changed in each version. Current builds target Windows x64.

## Sample courses

Ready-to-open example Course Packages are published in
[Cynapse-courses](https://github.com/albyho/Cynapse-courses), one release per
course. Download a zip, extract it to get a `<course-name>/` directory, and open
that directory in the Client; it then walks you through the real application,
step by step, while checking your work as you go.

From `0.9.0` on, the Client can also download a course zip straight from a URL,
so the whole step happens inside the app.

| Course | Version | Package | Based on |
| --- | --- | --- | --- |
| Rust by Example (Chinese) | `0.20.31` | [zip](https://github.com/albyho/Cynapse-courses/releases/download/rust-by-example-zh-cn-0.20.31/rust-by-example-zh-cn-0.20.31.zip) | [Rust by Example (Chinese)](https://rustwiki.org/zh-CN/rust-by-example/) |
| agent-desktop Source Analysis (Chinese) | `0.39.2` | [zip](https://github.com/albyho/Cynapse-courses/releases/download/agent-desktop-source-analysis-zh-cn-0.39.2/agent-desktop-source-analysis-zh-cn-0.39.2.zip) | [agent-desktop](https://github.com/vercel-labs/agent-desktop) |
| Signal Processing Roadmap (Chinese) | `0.2.5` | [zip](https://github.com/albyho/Cynapse-courses/releases/download/signal-processing-roadmap-zh-cn-0.2.5/signal-processing-roadmap-zh-cn-0.2.5.zip) | [Signal processing roadmap](https://github.com/minthanthtoo/signal-processing-roadmap) |
| Signal Processing Roadmap (English) | `0.2.5` | [zip](https://github.com/albyho/Cynapse-courses/releases/download/signal-processing-roadmap-0.2.5/signal-processing-roadmap-0.2.5.zip) | [Signal processing roadmap](https://github.com/minthanthtoo/signal-processing-roadmap) |

> These are samples, not a finished catalogue. `rust-by-example-zh-cn` is an
> in-progress adaptation that currently covers the introduction and chapters 1–4
> only, not the complete Rust by Example book.
> `signal-processing-roadmap-zh-cn` is an in-progress adaptation that currently
> covers the introduction and Week 1 only, not the complete roadmap.

## Roadmap

- Ship `Cynapse Studio`, the course authoring application.
- Add more sample courses.
- Add AI agent support.
- Add Accessibility Tree support.
- Add a TurboWarp (Scratch) Adapter.
- Add a Blender Adapter.
- Add more UI languages and course-content localization (i18n / L10n).

## Layout

| Path | Contents |
| --- | --- |
| `client/` | GPUI Client: `core/` (protocol server and state machines), `model/` (application state and use cases), and `app/` (windows and platform integration) |
| `course/` | Shared `cynapse-course` Course Package contract |
| `adapters/` | App Adapters: the VS Code extension and the JupyterLab prebuilt extension |
| `protocol/` | Wire contract and shared protocol samples |
| `packaging/` | MSIX packaging template and icon generation |
| `scripts/` | Build, packaging, and verification scripts |

## Build and test

Requires Node 24, pnpm 12, Rust 1.100+, and Python 3. Building the Client on
Windows also needs the MSVC toolchain and CMake (GPUI prerequisites); run it from
a Developer PowerShell or an equivalent environment.

```powershell
pnpm install
pnpm dev:client        # start the Client
pnpm build:client      # release build of the Client
pnpm build:adapter     # compile the VS Code Adapter
```

```powershell
pnpm verify:client     # Client gate: fmt, clippy, tests
pnpm verify:adapter    # Adapter packages: format and tests
pnpm verify:course     # Course package contract
pnpm verify:commit     # fast checks: line endings, docs, scripts, versions
```

The JupyterLab Adapter builds through `adapters/jupyterlab`; install
`requirements-build.txt` there before building the extension wheel.

## Issues

Bug reports and questions are welcome in the issue tracker.

## License

MIT. See [LICENSE](LICENSE).
