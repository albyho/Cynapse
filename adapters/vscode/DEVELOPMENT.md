# Cynapse VS Code Adapter Development

开发、测试和打包 VS Code Adapter 的说明。面向用户的安装与使用文档见
[README.md](README.md)。

## 架构

Adapter 负责通过本地 WebSocket 连接单例 Cynapse Client，并完成协议握手。
主要代码位于 `src/`：

- `adapterConnection.ts` — WebSocket 连接、握手、重连和 ACK。
- `apply.ts`、`vscodeFacade.ts` — 在真实编辑器中执行课程动作。
- `terminal.ts`、`terminalFacade.ts` — Cynapse 终端会话与命令执行。
- `discovery.ts` — 读取 Client 发布的本地运行时发现文件。

协议实现必须与根目录 `protocol/schema.json` 保持一致。

## 开发

在仓库根目录执行：

```powershell
nvm use 24.22.0
pnpm install
pnpm build:adapter
```

也可以在 `adapters/vscode` 中打开 VS Code 并使用
**Run Cynapse VS Code Adapter** 调试配置。

## 测试

```powershell
pnpm verify:adapter
```

测试使用共享的 Protocol Schema 和测试夹具，以及 Adapter 自有的测试数据。
测试不会读取仓库级 `courses/`；测试路径在运行时推导，不固定为
某台机器上的绝对路径。

测试必须确定性：用固定的无效 PID 或端口、注入的时钟或判断函数，不要依赖真实
子进程、真实时间或外部服务。临时目录使用 `os.tmpdir()`，测试之间不共享状态。

适配器的验证门禁是 `pnpm verify:adapter`，即 `tsc` 编译加 `node --test`；
当前没有 ESLint/Prettier 门禁，新增格式规则前先补配置。

改动运行时发现路径、协议字段或行为后，grep `docs/`、`spec.md`、ADR 和手工
验收文档中的旧路径与旧描述，并同步更新。

## 打包

每次打包前都必须先递增 `package.json` 中的版本号。打包与后续发布必须使用同一个
新版本；不得重新打包或发布 Marketplace 上已经存在的版本。

```powershell
pnpm package:adapter
```

VSIX 输出到：

```text
adapters/vscode/dist/cynapse-vscode-adapter.vsix
```

`.env`、`dist/`、源码与测试数据不会进入 VSIX。

## 发布流程

扩展发布到 VS Code Marketplace，发布者 ID 为 `tubumu`。

### 一次性配置

1. 在 [VS Code Marketplace 管理页](https://marketplace.visualstudio.com/manage)
   创建发布者 `tubumu`。
2. 在 Azure DevOps 创建 Personal Access Token：
   - Organization 选择 **All accessible organizations**。
   - Scopes 选择 **Custom defined → Marketplace → Manage**。
3. 将 Token 写入本地 `adapters/vscode/.env`：

```text
VSCE_PAT=<Azure DevOps PAT>
```

`.env` 已被 Git 和 VSIX 打包规则排除。不要提交、上传或分享其中的 Token。

### 发布新版本

1. 在打包前递增 `adapters/vscode/package.json` 中的版本。Marketplace 不接受重复
   版本号，打包和发布必须使用高于线上版本的新版本。
2. 在仓库根目录验证并打包：

```powershell
nvm use 24.22.0
pnpm verify:adapter
pnpm package:adapter
```

3. 加载本地 Token 并发布：

```powershell
cd adapters/vscode

$env:VSCE_PAT = (
  Get-Content .env |
  Where-Object { $_ -match '^VSCE_PAT=' } |
  ForEach-Object { ($_ -split '=', 2)[1] }
)

pnpm exec vsce publish --packagePath dist/cynapse-vscode-adapter.vsix
```

4. 发布成功后检查 Marketplace 页面，等待公共索引更新：

```text
https://marketplace.visualstudio.com/items?itemName=tubumu.cynapse-vscode-adapter
```

## 当前限制

Client 与 Adapter 的运行时发现路径目前以 Windows 为主。macOS 与 Linux 的
平台标准目录解析尚未统一，因此暂时不应将这些平台视为已支持。
