# Cynapse

[English](README.md) | 简体中文

Cynapse（Code + Synapse）是本地优先的交互式软件教学与工作流平台。一门课程包驱动
交互界面、可恢复的步骤状态机和一个或多个应用适配器，学员在真实软件里跟着操作，
而不是看录屏或在模拟器里练。

Windows 是当前支持的平台。客户端核心、通信契约与应用适配器都不绑定具体平台，
后续可以在不改课程契约的前提下支持更多平台。

[介绍视频](https://www.bilibili.com/video/BV1BSsD6MEaY/)

## 目录结构

| 路径 | 内容 |
| --- | --- |
| `client/` | GPUI 客户端：`core/`（协议服务器与状态机）、`model/`（应用状态与用例）、`app/`（窗口与平台集成） |
| `course/` | 共享的 `cynapse-course` 课程包契约 |
| `adapters/` | 应用适配器：VS Code 扩展与 JupyterLab 预构建扩展 |
| `protocol/` | 通信契约与共享协议样例 |
| `packaging/` | MSIX 打包模板与图标生成 |
| `scripts/` | 构建、打包与校验脚本 |

## 示例课程

可直接打开的示例课程包发布在
[Cynapse-courses](https://github.com/albyho/Cynapse-courses)，一门课一个 Release。
下载 zip，解压得到 `<course-name>/` 目录，在客户端里打开该目录即可；之后它会在真实
软件中一步步引导你操作，并随时检查结果。

从 `0.9.0` 起，客户端也支持直接从 URL 下载课程 zip，整个步骤都能在应用内完成。

| 课程 | 版本 | 课程包 | 改编自 |
| --- | --- | --- | --- |
| Rust by Example（中文） | `0.20.21` | [zip](https://github.com/albyho/Cynapse-courses/releases/download/rust-by-example-zh-cn-0.20.21/rust-by-example-zh-cn-0.20.21.zip) | [Rust by Example（中文）](https://github.com/rust-lang-cn/rust-by-example-cn) |
| agent-desktop 源码分析（中文） | `0.39.0` | [zip](https://github.com/albyho/Cynapse-courses/releases/download/agent-desktop-source-analysis-zh-cn-0.39.0/agent-desktop-source-analysis-zh-cn-0.39.0.zip) | [agent-desktop](https://github.com/vercel-labs/agent-desktop) |
| 信号处理路线图（中文） | `0.2.4` | [zip](https://github.com/albyho/Cynapse-courses/releases/download/signal-processing-roadmap-zh-cn-0.2.4/signal-processing-roadmap-zh-cn-0.2.4.zip) | [Signal processing roadmap](https://github.com/minthanthtoo/signal-processing-roadmap) |
| 信号处理路线图（英文） | `0.2.4` | [zip](https://github.com/albyho/Cynapse-courses/releases/download/signal-processing-roadmap-0.2.4/signal-processing-roadmap-0.2.4.zip) | [Signal processing roadmap](https://github.com/minthanthtoo/signal-processing-roadmap) |

> 这些是示例，不是完整的课程目录。`rust-by-example-zh-cn` 目前只包含简介和第
> 1–4 章，并非完整的《通过例子学 Rust》；`signal-processing-roadmap-zh-cn` 目前只
> 包含简介和第 1 周，尚未覆盖完整路线图。

## 构建与测试

需要 Node 24、pnpm 12、Rust 1.100+ 与 Python 3。在 Windows 上构建客户端还需要
MSVC 工具链与 CMake（GPUI 的前置条件），请在 Developer PowerShell 或等价环境里
运行。

```powershell
pnpm install
pnpm dev:client        # 启动客户端
pnpm build:client      # 客户端 release 构建
pnpm build:adapter     # 编译 VS Code 适配器
```

```powershell
pnpm verify:client     # 客户端门禁：fmt、clippy、测试
pnpm verify:adapter    # 适配器包：格式与测试
pnpm verify:course     # 课程包契约
pnpm verify:commit     # 快速检查：行尾、文档、脚本、版本
```

JupyterLab 适配器在 `adapters/jupyterlab` 下构建；构建扩展前先安装那里的
`requirements-build.txt`。

## 下载

| 组件 | 下载 |
| --- | --- |
| Cynapse Client + JupyterLab Adapter（Windows x64） | [`cynapse-<version>-windows-x64.zip`](https://github.com/albyho/Cynapse/releases/latest) |
| Cynapse VS Code Adapter | [VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=tubumu.cynapse-vscode-adapter) |

每次构建都把客户端与匹配版本的 JupyterLab 适配器 wheel 打在一起，不需要单独安装
适配器。各版本变化见[版本记录](CHANGELOG.md)。当前构建面向 Windows x64。

## 问题反馈

缺陷与问题请在 issue 中提出。

## 许可

MIT，见 [LICENSE](LICENSE)。
