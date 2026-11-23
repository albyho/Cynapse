# Cynapse Client

Cynapse Client 由三层 Rust 代码组成：

- `core/`：协议服务、Course Session、播放状态机、SQLite 进度、窗口几何恢复。
  不依赖任何 GUI 宿主，可用 `cargo test` 直接测试。
- `model/`：应用状态、用例编排，以及 `SessionService`（Tokio runtime + `CourseSession` +
  协议 server）。不依赖 GPUI、窗口或音频设备。
- `app/`：GPUI 宿主（`gpui-kit`）。只负责视图、窗口、平台集成和 rodio 音频适配。

没有 WebView、IPC command、前端状态副本或 `ts-rs` 生成层。

## 命名

- `ClientModel`：应用层模型，放在 `client/model`，不依赖 GPUI。
- `ClientViewModel`：GPUI 可观察实体，放在 `client/app/src/view_model.rs`；只持有
  `ClientModel` 和视图可读的派生值。
- `MainView` / `SettingsView` / `CaptionView`：声明式视图，放在 `client/app/src/views/`；
  只实现 `Render`，不做 IO、不持有业务状态。
- `platform::*`：窗口、托盘、单实例、字幕窗口等 OS 副作用；放在
  `client/app/src/platform/`。
- 主窗口标题栏使用 GPUI Kit `TitleBar`（自绘、跨平台），不要自绘第二套窗口按钮。
- 不使用 `ClientApp`、`Shell`、`Host`、`Presenter`、`Controller`、`Command` 作为类型名。

## 构建与运行

```sh
cargo run -p cynapse-client
cargo test -p cynapse-client-core -p cynapse-client-model -p cynapse-client
```

Windows 构建需要 Visual Studio 的 MSVC 工具链与 CMake（GPUI 的前置条件），
并在 Developer PowerShell 或等价环境里运行。
