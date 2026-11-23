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

## 课程

课程包发布在 [Cynapse-courses](https://github.com/albyho/Cynapse-courses)，一门课
一个 [Release](https://github.com/albyho/Cynapse-courses/releases)。在客户端里
载入课程，它就会在真实软件中一步步引导你操作，并随时检查结果。

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

打包产物发布在[发布页](https://github.com/albyho/Cynapse/releases)，每次构建包含
客户端与匹配版本的 JupyterLab 适配器 wheel。VS Code 适配器通过 Visual Studio
Marketplace 分发。当前构建面向 Windows x64。

## 问题反馈

缺陷与问题请在 issue 中提出。
