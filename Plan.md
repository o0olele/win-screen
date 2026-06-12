User has approved your plan. You can now start coding. Start with updating your todo list if applicable

Your plan has been saved to: C:\Users\rick\.claude\plans\happy-singing-hanrahan.md
You can refer back to it if needed during implementation.

## Approved Plan:
# win-screen:Windows 截屏 / 录屏 / 桌面贴图 Rust 库

## Context(背景与目标)

在空目录 `D:\Code\chatting\win-screen` 从零搭建一个 Rust 库,主打 **Windows**、高性能、现代化,提供三大能力:

1. **截屏** —— 完整交互式工具:框选区域 + 标注(箭头/矩形/椭圆/文字/画笔/马赛克/序号)+ 复制/保存,体验对标 Snipaste / QQ 截图。
2. **录屏** —— MP4(H.264 硬件编码)+ 系统声音 + 麦克风混音。
3. **桌面贴图** —— 截图后钉在桌面的置顶小窗,可拖动/缩放/调透明度/复制/保存/多开。

交付形态(用户已确认):
- **Rust crate + Tauri 插件** —— 直接被 Tauri 即时通信项目导入(commands + events)。
- **独立可执行程序** —— 单独运行,便于测试,也可作为子进程被任意宿主调用。

UI 渲染(已确认):**原生 Win32 分层窗口**(最轻量、性能最好、零多余依赖)。
关键收益:遮罩层和贴图窗都是原生 Win32,**完全独立于 Tauri 的 WebView**,从根本上避开 `tauri-webview2-browser-args-mismatch` 那个次窗口不显示的坑。

---

## 总体架构:Cargo Workspace

核心库与 Tauri 解耦,保证 core 不依赖 tauri,可被任意宿主复用。

```
win-screen/
├── Cargo.toml                  # [workspace]
├── crates/
│   ├── win-screen-core/        # 纯 Rust 核心(rlib):截图/录屏/贴图/遮罩/标注/IO,不依赖 tauri
│   ├── win-screen-tauri/       # Tauri 插件(rlib):commands + events,薄封装 core
│   └── win-screen-cli/         # 独立 exe(bin):shot / record / pin 子命令,验证 core
└── examples/tauri-demo/        # 最小 Tauri 集成示例(验证插件)
```

### core 模块划分(`win-screen-core/src/`)

| 模块 | 职责 |
|------|------|
| `platform/`   | Win32 公共设施:窗口创建、消息循环、分层窗口 blit、DPI 感知、虚拟屏几何、`RegisterHotKey` 全局热键、COM 初始化 |
| `capture/`    | 截图引擎:全屏 / 指定显示器 / 指定窗口(HWND)/ 指定区域,基于 WGC 取单帧 → RGBA buffer |
| `overlay/`    | 交互式框选遮罩:跨所有显示器的全屏透明分层窗,暗化背景 + 拖拽选框 + 尺寸/放大镜 + 窗口高亮探测 + ESC/Enter |
| `annotate/`   | 标注编辑器:矩形/椭圆/箭头/直线/画笔/文字/荧光笔/马赛克模糊/序号 + undo/redo 栈,在选区快照上做矢量叠加 |
| `pin/`        | 桌面贴图:无边框置顶分层窗显示图像,拖动/缩放/透明度/复制/保存/关闭,多贴图管理 |
| `record/`     | 录屏:WGC 帧 → MP4 视频;WASAPI 采集系统 loopback + 麦克风,混音后写入;start/pause/resume/stop |
| `io/`         | 剪贴板(图像)、PNG/JPG 保存、编码工具 |
| `api.rs`      | 高层公开 API + 类型(配置、事件),channel/回调驱动 |
| `error.rs`    | `thiserror` 统一错误 |

---

## 技术选型(crate)

| 用途 | crate | 说明 |
|------|-------|------|
| Win32 / Direct2D / DirectWrite / DXGI / DWM | `windows`(官方) | 窗口、分层窗、热键、剪贴板、D2D 渲染 |
| WGC 采集 + MP4 编码 | `windows-capture` 1.5+ | 截图单帧 `frame.buffer()` + 录屏 `VideoEncoder`(底层 MF 硬件 H.264) |
| 音频 loopback + 麦克风 + AEC | `wasapi` | 系统声 loopback 流 + 麦克风流,双流混音 |
| 图像编码/缓冲 | `image` | RGBA buffer、PNG/JPG 编码 |
| 剪贴板(含图像) | `arboard` | 跨平台剪贴板,支持图像写入 |
| 全局热键(可选) | `global-hotkey` | 截图/录屏快捷键 |
| 错误/事件/序列化 | `thiserror`、`crossbeam-channel`、`serde` | 错误、跨线程事件、Tauri 序列化 |

**遮罩层与贴图窗的窗内渲染**:推荐 **Direct2D + DirectWrite**(GPU 抗锯齿、文字清晰、与 WGC 同处 DX 体系,符合"现代+高性能");分层窗用 `UpdateLayeredWindow` 呈现。GDI/GDI+ 作为兜底备选。

---

## 公开 API 草图(core)

```rust
// 截图
Screenshot::capture_fullscreen() -> CapturedImage
Screenshot::capture_monitor(id) / capture_window(hwnd) / capture_region(rect)

// 交互式截图(遮罩 + 标注),返回最终图像或被取消
Capturer::interactive(opts) -> Option<CapturedImage>

// 录屏
Recorder::builder().target(..).audio(system, mic).output(path).start() -> RecordingHandle
handle.pause() / resume() / stop()

// 桌面贴图
Pin::from_image(img) -> PinHandle
Pin::from_clipboard() -> PinHandle
handle.close() / set_opacity(..)
```

事件(框选完成、录屏结束、贴图关闭)通过 `crossbeam-channel` 或回调回传给宿主。

### 线程 / COM 模型(关键约束)
- 每个 Win32 窗口(遮罩、各贴图)需在带 `GetMessage` 消息泵的线程上;遮罩/窗口用 **STA**。
- WGC 采集线程由 `windows-capture` 自管;WASAPI 音频用独立线程且 **MTA**(不可在 UI 线程 `initialize_mta`)。
- 公开 API 通过 channel 把结果回传调用方(Tauri/CLI),不阻塞宿主主线程。

---

## Tauri 插件(`win-screen-tauri`)

- `tauri::plugin::Builder` 注册 commands:`capture_fullscreen` / `start_interactive_capture` / `start_recording` / `stop_recording` / `pin_image` / `pin_from_clipboard` / `list_pins` / `close_pin`。
- 向前端 emit 事件:`win-screen://capture-done`、`win-screen://recording-stopped`。
- 图像回传:临时文件路径 + Tauri asset 协议,或 base64(小图)。
- 遮罩/贴图为原生 Win32,**不创建 Tauri WebviewWindow**,规避 WebView2 次窗口坑。
- 热键交给宿主(或本插件可选注册)。

## 独立 exe(`win-screen-cli`)
- `win-screen shot [--interactive|--fullscreen] [--save path|--clipboard]`
- `win-screen record --output out.mp4 [--audio system,mic]`
- `win-screen pin [--file img.png|--clipboard]`
- 作用:不依赖 Tauri 即可验证 core 全部能力。

---

## 分阶段实现

- **P0 脚手架**:workspace + 三个 crate + 依赖接入,`cargo build` 通过。
- **P1 截图引擎**:全屏/显示器/窗口/区域截图 → PNG + 剪贴板;CLI `shot` 验证。
- **P2 交互遮罩**:Win32 全屏分层遮罩、多屏、暗化、拖拽选框、放大镜、窗口探测、ESC/Enter。
- **P3 标注编辑器**:工具栏 + 各标注工具 + undo/redo,定稿 → 剪贴板/保存。
- **P4 桌面贴图**:原生置顶窗,拖动/缩放/透明度/复制/保存/关闭/多开。
- **P5 录屏**:先 WGC→MP4 纯视频跑通;再加 WASAPI 系统+麦克风混音;start/pause/stop。
- **P6 Tauri 插件**:commands/events 封装 + `examples/tauri-demo` 集成验证。
- **P7 打磨**:DPI 边界、全局热键、配置项、错误处理、性能 pass。

## 主要风险点
- **录屏混音**:WGC 自带音频通常只有系统声;系统声+麦克风需自行用 `wasapi` 双流采集→重采样→混合→喂给编码器(或用 Media Foundation 单独 mux)。策略:P5 先视频跑通,再叠加音频,降低风险。
- **高 DPI / 多显示器**:坐标系统一到虚拟屏 + per-monitor DPI;遮罩与贴图都要按物理像素对齐。
- **Win10 兼容**:WGC 某些设置(无边框 border)在旧 Win10 不支持,需运行期降级处理。

---

## 验证方式
- `cargo build` 整个 workspace 通过。
- CLI 逐能力手测:`cargo run -p win-screen-cli -- shot --interactive` / `... record ...` / `... pin --clipboard`。
- 多显示器 + 高 DPI 手动验证遮罩选框与贴图像素对齐。
- `examples/tauri-demo` 跑起来,前端按钮触发各 command,确认事件回传与图像显示正常。
- 录屏产物用播放器验证画面+声音(系统声、麦克风均在)。
