# Cynapse VS Code Adapter

[English](README.md) | 简体中文

在 VS Code 中运行 Cynapse 交互课程与工作流的本地扩展。

Cynapse 将软件操作、代码编辑与终端命令组织为可逐步学习的交互式课程。安装此扩展后，Cynapse Client 可以在真实 VS Code 工作区中打开文件、引导代码编辑，并使用受管理的终端执行课程命令。

> 此扩展是 Cynapse Client 的配套组件，不能单独播放课程。

## 功能

- 自动发现并连接本机正在运行的 Cynapse Client。
- 连接断开后自动重试，并在 VS Code 状态栏显示当前状态。
- 配合 Client 打开课程工作区、打开或创建文件。
- 在真实编辑器中执行提示、打字和代码编辑操作。
- 使用 Cynapse 管理的终端运行课程命令并读取结果。
- 在课程重置时安全关闭相关标签页和终端。

## 环境要求

- VS Code `1.140.0` 或更高版本。
- 已安装并启动 Cynapse Client。
- Cynapse Client 与 VS Code Adapter 必须运行在同一个用户会话中。

当前 Cynapse Client 以 Windows 为主要支持平台，其他平台仍在完善中。

## 开始使用

1. 从 VS Code Marketplace 安装 **Cynapse VS Code Adapter**。
2. 启动 Cynapse Client。
3. 在 Client 中打开课程并开始学习。
4. 扩展会自动连接本地 Client，并按课程要求打开工作区。

扩展会在后台运行，无需每次手动启动。VS Code 状态栏右侧会显示以下状态：

| 状态 | 含义 |
| --- | --- |
| `Cynapse: 等待` | 已加载扩展，正在等待 Client 或重新连接 |
| `Cynapse: 已连接` | 已成功连接 Cynapse Client |
| `Cynapse: 错误` | 连接或课程操作失败 |

状态栏与命令标题跟随 VS Code 的显示语言。本扩展提供英文和简体中文。

如需查看详细状态，打开命令面板并运行：

```text
Cynapse: 显示 VS Code Adapter 状态
```

英文界面下为 `Cynapse: Show VS Code Adapter Status`。

## 故障排查

### 一直显示“等待”

- 确认 Cynapse Client 已经启动。
- 确认 VS Code 与 Client 运行在同一个用户会话中。
- 执行 **Developer: Reload Window** 后重新检查。

### 显示连接或执行错误

1. 运行 `Cynapse: Show VS Code Adapter Status` 查看错误信息。
2. 重新启动 Cynapse Client。
3. 如果问题持续，提交 [Issue](https://github.com/albyho/Cynapse/issues)，并附上 VS Code 版本、操作系统版本和状态信息。

## 安全与隐私

- 扩展与 Cynapse Client 只通过 `127.0.0.1` 上的本地连接通信。
- 此扩展不包含远程遥测或上传课程内容的逻辑。
- 课程可以修改工作区文件和运行终端命令，请仅在可信课程和专用工作区中使用。
- 执行终端命令前，请确认命令来源可信，并保留必要的版本控制或文件备份。

## 关于 Cynapse

Cynapse（Code + Synapse）是一个本地优先、面向真实软件的交互式教学与工作流平台。课程包驱动交互界面、可恢复的分步状态机，搭配一个或多个应用程序适配器，让学习者直接在软件里跟着做，而不是看录屏，也不是在模拟器里练。整个过程可观察、可暂停、可恢复。

发布与问题反馈：
[github.com/albyho/Cynapse](https://github.com/albyho/Cynapse)

## License

[MIT](LICENSE)
