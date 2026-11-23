# Cynapse Client Core

`client/core` 是 Cynapse Client 的 Rust 核心库。它不包含 GPUI 窗口或视图，负责课程解析、会话状态、播放调度、协议、持久化与环境检测。无界面场景验证与内容校验在并列的 [`client/harness`](../harness) crate 中。

上层 [client](../) 通过公开 API 调用本库；Adapter 通过本地 WebSocket 与这里的 Server 通信。

## 主要模块

| 模块 | 职责 |
| --- | --- |
| [`client_state`](src/client_state.rs) | 把会话、学习、播放和过渡状态归约为 Client 的单一份状态快照 |
| [`course`](src/course.rs) | 加载并校验 Course Package、Course Tree、Step、Action、资源和 Narration Clip |
| [`course_session`](src/course_session.rs) | 打开课程、选择节点、管理课程进度、Workspace、重置和恢复 |
| [`client_transition`](src/client_transition.rs) | 串行化 Start、Stop 等独占 Client Transition，并维护 Stop 安全屏障 |
| [`learning`](src/learning.rs) | 按步学习与连续学习的运行时状态 |
| [`server`](src/server.rs) | 本地 WebSocket Server、Action 调度、暂停、重试和 Narration 门控 |
| [`protocol`](src/protocol.rs) | Client 与 Adapter 之间的 Envelope、Action、Ack 和 Cancel 契约 |
| [`environment`](src/environment.rs) | 环境检查结果与版本约束 |
| [`discovery`](src/discovery.rs) | 单例运行时发现文件和 SQLite 状态路径 |
| [`window_position`](src/window_position.rs) | 多显示器可见性、窗口位置约束和持久化 |

本 crate 只提供库，不含可执行入口。`cynapse-scenario` 与
`cynapse-content-verify` 位于 [`client/harness`](../harness)，那里也是 Scenario
运行器与假文件系统的家；本 crate 不依赖它，生产路径因此不包含任何验证设施。

## 测试组织

测试按访问边界放置：

- 需要访问模块私有实现的单元测试，放在所属模块旁的 `tests.rs` 或 `tests/` 目录，例如 [`window_position/tests/`](src/window_position/tests/)。生产源码只保留 `#[cfg(test)] mod tests;` 声明。
- 只通过公开 API 验证行为的集成测试，放在 [`tests/`](tests) 中。例如协议契约测试位于 [`tests/protocol.rs`](tests/protocol.rs)。
- 需要驱动整门课程场景的测试放在 [`client/harness/tests/`](../harness/tests)，走 `cynapse-client-harness` 的公开 API。
- `tests/` 下的测试不应依赖组件私有的实现细节。

## 关键约束

- Course Tree 只有分组节点和节点；节点是可选择、可持久化学习状态的节点。
- Course Package 在打开阶段完成结构校验；不要绕过 `course` 模块直接解释 Manifest 或 Step。
- Stop、失败和重试以 Step 为恢复边界，重试从该 Step 的第一个 Action 重新执行。
- Action、环境检查和终端操作必须经受控 Protocol 发送给 Adapter，由 Adapter 执行真实软件操作。
- Narration 是操作辅助；旧 `step_run_id` 的异步媒体事件必须忽略。
- 课程进度、Workspace 和 SQLite 的变更必须通过 `course_session` 的公开接口完成。

## Loader 边界

- `course` 的加载阶段校验包结构、引用安全、Action 字段与组合限制、媒体结构，
  以及编辑 Step 必须先 `OpenFile` 的静态自包含契约。
- 加载阶段不检查真实 Workspace 文件是否存在、编辑器是否打开或 dirty、literal
  实际匹配情况、行列边界、环境、终端和命令结果。
- 这些运行时条件由 `course_session`、Server 和 Adapter 在执行或恢复当前 Step
  时检查；失败时保留当前 Step，不能由 Loader 推断课程已经完成。
- Caption emphasis markup 由内容验证和 Narration Assembler 检查，不由
  `load_package` 自身执行。

## 验证

在仓库根目录运行：

```powershell
# 仅验证 Core
cargo test -p cynapse-client-core
cargo clippy -p cynapse-client-core --all-targets -- -D warnings

# 通过仓库脚本验证 Core、harness 及其内容约束
pnpm verify:client
pnpm verify:content

# 运行确定性课程场景
pnpm run:scenario --course courses/rust-by-example-zh-cn --unit introduction --mode continuous
```

开发迭代按改动范围选择：只改 Client 时运行 `pnpm verify:client`，只改
`cynapse-course` 时运行 `pnpm verify:course`；共享课程库改动会同时影响两个应用，
运行 `pnpm verify:course:dependents`。完整的 `pnpm verify:push` 留到 merge/release
前或 CI 不可用时。
