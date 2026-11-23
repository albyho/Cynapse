# Cynapse VS Code Adapter

English | [简体中文](README.zh-CN.md)

The local extension that runs Cynapse interactive courses and workflows inside
VS Code.

Cynapse organizes software actions, code edits, and terminal commands into
interactive courses you learn step by step. With this extension installed,
Cynapse Client can open files in a real VS Code workspace, guide code edits,
and run course commands in managed terminals.

> This extension is a companion to Cynapse Client and cannot play a course on
> its own.

## Features

- Automatically discovers and connects to a local Cynapse Client.
- Retries after disconnects and shows the current state in the VS Code status
  bar.
- Opens the course workspace and opens or creates files with the Client.
- Performs hints, typing, and code edits in the real editor.
- Runs course commands in Cynapse-managed terminals and reports their results.
- Closes related tabs and terminals safely when a course is reset.

## Requirements

- VS Code `1.140.0` or newer.
- Cynapse Client installed and running.
- Cynapse Client and the VS Code Adapter must run in the same user session.

Cynapse Client is currently Windows-first; other platforms are still being
completed.

## Getting started

1. Install **Cynapse VS Code Adapter** from the VS Code Marketplace.
2. Start Cynapse Client.
3. Open a course in the Client and start learning.
4. The extension connects to the local Client automatically and opens the
   workspace required by the course.

The extension runs in the background; you do not need to start it each time.
The right side of the VS Code status bar shows:

| State | Meaning |
| --- | --- |
| `Cynapse: Waiting` | The extension is loaded and waiting for the Client or reconnecting |
| `Cynapse: Connected` | Connected to Cynapse Client |
| `Cynapse: Error` | The connection or a course action failed |

The status bar and command titles follow VS Code's display language. This
extension ships English and Simplified Chinese strings.

To inspect the detailed state, open the Command Palette and run:

```text
Cynapse: Show VS Code Adapter Status
```

## Troubleshooting

### It stays on "Waiting"

- Confirm Cynapse Client is running.
- Confirm VS Code and the Client run in the same user session.
- Run **Developer: Reload Window** and check again.

### It shows a connection or execution error

1. Run `Cynapse: Show VS Code Adapter Status` to see the error.
2. Restart Cynapse Client.
3. If the problem persists, open an
   [issue](https://github.com/albyho/Cynapse-release/issues) with your VS Code
   version, OS version, and the status details.

## Security and privacy

- The extension and Cynapse Client communicate only over a local connection on
  `127.0.0.1`.
- The extension contains no remote telemetry or course-content upload.
- Courses can modify workspace files and run terminal commands. Use only
  trusted courses and dedicated workspaces.
- Before running terminal commands, confirm the source is trusted and keep
  version control or file backups.

## About Cynapse

Cynapse is a local-first interactive software tutorial and workflow platform.
It lets you learn in real software while keeping the process observable,
pausable, and resumable.

Releases and issue tracker:
[github.com/albyho/Cynapse-release](https://github.com/albyho/Cynapse-release)

## License

[MIT](LICENSE)
