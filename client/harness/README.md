# Cynapse Client Harness

`client/harness` 是 Cynapse Client 的验证工具 crate，不进入生产路径。它用脚本化
Adapter 通过真实 WebSocket 驱动整门课程，抓取产物，并提供两个可执行入口：

- `cynapse-scenario`：无界面运行 Course Package 场景。
- `cynapse-content-verify`：验证 Content Package 及其 Caption 等结构约束。

上层 [client](../) 与应用运行时的库是 [`client/core`](../core)；本 crate 只单向依赖
它，且**只使用其公开 API**——需要私有实现才能验证的事，说明该行为还没有公开契约。

## 主要模块

| 模块 | 职责 |
| --- | --- |
| [`lib`](src/lib.rs) | Scenario 入口点，串起 Session、Server 与脚本化 Adapter |
| [`runner`](src/runner.rs) | `run_scenario` 家族与 Adapter 执行循环 |
| [`types`](src/types.rs) | Spec、Report、Error 与 `ClientSocket` 别名 |
| [`fake_fs`](src/fake_fs.rs) | 假文件系统，复用 `cynapse-course` 的编辑与文件操作语义 |
| [`artifacts`](src/artifacts.rs) | 报告落盘与复现命令生成 |

## 验证

```powershell
# 场景运行器与内容校验自身的测试
cargo test -p cynapse-client-harness
pnpm verify:client

# 对真实课程包跑内容校验
pnpm verify:content -- --course courses/<包>
pnpm run:scenario --course courses/<包> --unit <节点> --mode continuous
```

`cli` 测试通过 `CARGO_BIN_EXE_cynapse-scenario` 调用同包的可执行文件，因此可执行入口
与这些测试必须留在同一个 crate。

## 测试组织

- [`tests/scenario_runner.rs`](tests/scenario_runner.rs)：整门课的黑盒场景测试，
  子模块按行为分（actions、scripting、modes、continuous、narration、cli、challenge）。
- [`tests/recovery_runner.rs`](tests/recovery_runner.rs)：编辑 Action 的恢复自愈。
- 共享的课程夹具构造在 [`tests/common/`](tests/common)，不依赖 `courses/` 下的真实内容包。
