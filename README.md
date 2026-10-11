# AITools

用于集中管理由 AI 接管、构建和扩展的工具。

## 当前工具

- `QTranslate/`：跨应用划词翻译和屏幕 OCR 工具，源码来自 [ahatem/QTranslate](https://github.com/ahatem/QTranslate)。
- `BongoCat/`：桌面 Live2D 猫宠物覆盖层（源自 [ayangweb/BongoCat](https://github.com/ayangweb/BongoCat)），由 AI 接管构建与自定义魔改。该目录为**独立 git 仓库**，已被本仓库 `.gitignore` 忽略，不纳入 AITools 提交。

## QTranslate

源码目录：`QTranslate/`

源码构建要求 Java 17+；当前构建使用 Java 21，Kotlin 工具链会使用 Java 11 编译兼容目标。

```powershell
cd QTranslate
.\gradlew.bat build --console=plain --no-configuration-cache `
  '-Duser.language=en' '-Duser.country=US'
```

生成源码构建的便携包：

```powershell
.\gradlew.bat assembleReleaseVariants --console=plain --no-configuration-cache `
  '-PreleaseVersion=1.5.0'
```

产物位于 `QTranslate/build/release/`。本机已将便携包展开到 `QTranslate/dist/QTranslate-1.5.0/`，运行入口为其中的 `QTranslate/QTranslate.jar`。

运行数据和翻译历史保存在便携目录的 `logs/`、`plugins_data/` 等目录中，不参与 Git 提交。

## BongoCat

源码目录：`BongoCat/`（独立 git 仓库，不参与 AITools 的版本管理）。

纯 Rust + gpui-kit 原生 GUI，无 WebView / Node 依赖。

### 构建要求

- Rust 工具链：由 `rust-toolchain.toml` 锁定 `channel = "1.97.1"`（edition 2024），无需手动安装指定版本。
- 链接器：需 MSVC。构建前用 VS2022 的 `vcvarsall.bat x64` 初始化环境。
- `cargo` 路径：rustup 安装后需将 `C:\Users\<user>\.cargo\bin` 显式加入 `PATH`（当前 shell 不一定自动刷新）。
- crates.io 走 rsproxy 镜像（`~/.cargo/config.toml`）。
- 两个 GitHub git 依赖（ayangweb/gilrs、ayangweb/permission-flow）需可访问 GitHub。

### 运行

- 开发运行：`just dev`（等价于 `cargo run -p bongocat-app --release -- --run-seconds 0`）。
- 生产构建与打包：`just build`（调用 `bongocat-packaging`，产出可分发的安装包）。
- 仅类型检查：`cargo check -p bongocat-app --locked`。

### AI 自定义魔改

- **全屏自动隐藏**：当游戏 / 独占全屏视频等前台窗口覆盖整块显示器时，覆盖层淡出为透明并穿透鼠标点击（与 hover / idle 隐藏行为一致）。开关为配置项 `overlay.hide_on_fullscreen`（默认 `false`）。
  - 配置文件：`%APPDATA%\com.ayangweb.bongo-cat\development\config.json`（dev 构建）或 `production\config.json`（生产构建）的 `overlay` 段加入 `"hide_on_fullscreen": true`。
- **开发构建开放开机自启**：撤销上游 `docs/adr/0051` 的 dev 门禁（`crates/bongocat-app/src/settings/projection.rs` 的 `startup_item_available()` 现允许 Development），开发构建也能在系统中注册独立的 `BongoCat Development` 启动项，无需先打 Production 包。

### 注意事项

- **开机自启的开启方式**：`cargo run` / `just dev` 的开发构建现已支持「登录时启动」，在 BongoCat 设置的「登录时启动」中开启即可随系统启动；其注册项指向 `target/` 下的二进制，`cargo clean` 后会失效（个人使用可接受）。
- 状态栏 / 任务栏 / 托盘图标默认关闭（`show_taskbar_icon` / `show_status_icon` 为 `false`），运行时通常看不到常驻入口，属预期行为。
