# Cynapse

English | [简体中文](README.zh-CN.md)

Cynapse (Code + Synapse) is a local-first platform for interactive software
tutorials and workflows. A Course Package drives an interactive UI, a
recoverable step state machine, and one or more App Adapters, so learners follow
along in the real application instead of watching a recording or working through
a simulator.

Windows is the first supported platform. The Client core, the wire protocol, and
the App Adapters stay platform-neutral, so further platforms can be added
without changing the course contract.

[Introduction video](https://www.youtube.com/watch?v=fk4KJ7OfV-U)

## Layout

| Path | Contents |
| --- | --- |
| `client/` | GPUI Client: `core/` (protocol server and state machines), `model/` (application state and use cases), and `app/` (windows and platform integration) |
| `course/` | Shared `cynapse-course` Course Package contract |
| `adapters/` | App Adapters: the VS Code extension and the JupyterLab prebuilt extension |
| `protocol/` | Wire contract and shared protocol samples |
| `packaging/` | MSIX packaging template and icon generation |
| `scripts/` | Build, packaging, and verification scripts |

## Courses

Course packages are published in
[Cynapse-courses](https://github.com/albyho/Cynapse-courses), with one
[release](https://github.com/albyho/Cynapse-courses/releases) per course. Load a
course in the Client and it walks you through the real application, step by
step, while checking your work as you go.

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

## Download

Packaged builds are published on the
[releases page](https://github.com/albyho/Cynapse/releases). Each build bundles
the Client with the matching JupyterLab Adapter wheel; the VS Code Adapter is
distributed through the Visual Studio Marketplace. Current builds target Windows
x64.

## Issues

Bug reports and questions are welcome in the issue tracker.

## License

MIT. See [LICENSE](LICENSE).
