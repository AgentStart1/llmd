# 开发

工作区因 Ratatui 0.30 要求 Rust 1.88 或更新版本；Tauri 应用要求 Rust 1.90 或更新版本。更新 Tauri CLI 和 Rust crate 后，运行 `npm --prefix app run tauri -- android init --ci` 重新生成 Android，再运行 `scripts/sync-tauri-android-overrides.sh`。Tauri 2.12 生成 Gradle 9.6.1 / AGP 9.3.1 项目，目标为 Android API 37；Android 检查前先安装该 SDK 平台。维护中的 Android 库和 IPC 示例将 Java 与 Kotlin 编译到 JVM 17。在 `bundle.android.debugApplicationIdSuffix` 保留 debug 包名后缀，让 CLI 重写生成的 Gradle 文件时能保留它。更新 npm 依赖时，同步 `app/package-lock.json` 和 `app/pnpm-lock.yaml`。Dependabot 每周检查 GitHub Actions、`app` 的 npm、两个 Cargo 项目，以及 Android 库和 IPC 示例的 Gradle 依赖。版本更新按生态系统分组，包括主、次和补丁版本。生成的 Android 工具链版本仍由 Tauri 模板管理；Kotlin 覆盖配置在 `app/src-tauri/android/patches` 中维护。Android E2E 运行器的 Fantoccini action 类型必须与 `appium-client` 暴露的版本一致。0.2.2 要求 Fantoccini 0.19；在 Appium 客户端支持相应公开类型前，Dependabot 忽略 Fantoccini 0.20 及更新版本。兼容性限制与其 Dependabot 例外必须一起更新。

Android 源码归属 `app/src-tauri/android`。生成的 Tauri 工作区已忽略；Tauri 构建后、打包变体前，通过 `scripts/sync-tauri-android-overrides.sh` 应用权威 activity 覆盖配置，因为重新生成可能重置 application ID。前端资源嵌入 Rust 库，因此修改前端后必须执行 Tauri 构建；只用旧库重新执行 Kotlin Gradle 构建不会更新 UI。

`LlmdServiceHost` 在注入的串行调度器上管理 Off → Starting → Ready → Stopping → Off 生命周期，引擎操作在注入的 worker 上运行。应用单例跨 Activity 和 Binder 连接持有它。私有前台服务观察其状态，并提供持续可用的停止操作。Binder 客户端不能启动或替换模型。停止时取消提供方任务，等待会话清理后再关闭引擎。导入和删除要求服务已停止；导入保留原文件名，临时复制完成后才发布。

改动后运行 `scripts/check-static.sh --host --android`、`cargo test --workspace`、`cargo test --manifest-path app/src-tauri/Cargo.toml` 和 `git diff --check`。在生成的 Android 项目中运行 `:llmd-android:testDebugUnitTest`，验证生命周期和模型清单。测试覆盖显式选择、就绪、加载取消、资源释放、初始化失败时不回退，以及精确清单解析。

设备已连接时，在安装或测试前先取得设备侧测试锁。设备锁由调用方或 AI 插件在外部管理；测试入口可直接运行，不要求安装 agent 插件。运行 `scripts/test-android-appium.sh` 时明确设置 `GEMMA_MODEL_PATH`。`LLMD_ANDROID_E2E_SKIP_BUILD=1` 使用已安装的应用和示例 APK；`LLMD_ANDROID_E2E_IMPORT_ONLY=1` 检查导入、显式选择和服务就绪，不代表已覆盖 IPC 生成。会话强制启动选定应用，避免共存变体收到另一个变体的测试操作。运行器原地安装，不清除应用数据，也不允许版本降级。

## 后台运行

用户可见的 Binder 客户端应使用 `BIND_AUTO_CREATE | BIND_IMPORTANT` 绑定，让推理共享前台调用方的进程重要性。此举不会加载模型，也不能阻止系统因内存不足回收引擎。示例展示绑定方式；显式前台模型服务在断开连接后继续负责模型。

Android 13 及以上版本中，MainActivity 在分配所选引擎前请求通知权限。Activity 重建时保留待处理的显式模型 ID。Status UI 显示通知权限并可打开标准应用设置。部分厂商还要求允许后台运行，并在最近任务中保护或锁定 LLMD。这些系统选择与服务开关相互独立。

## 验证

Test 工作流在 pull request、与任务相关的 main 推送和手动触发时运行。main 上的运行建立 Rust 依赖缓存，后续 pull request 可恢复。Linux host 检查同时缓存根工作区的 `target` 和 `app/src-tauri/target`；Windows 打包仅缓存后者。Android debug 验证和 release 打包以不同键缓存 `app/src-tauri/target`。其目标产物使用不同 profile，不能互换；每份缓存还包含 Cargo registry/git 下载。缓存键按平台和构建用途隔离。Rust 缓存 action 保留依赖，不保留工作区输出或增量编译状态。声称节省时间前，应比较冷、热运行，并计入缓存恢复与保存耗时及大小。

真实 Binder 检查覆盖精确模型清单、缺失／空白／未知 ID、Off 状态下的有效 ID、Ready 状态下不匹配的 ID，以及断开后重新绑定而不加载或替换模型。显式 E4B 文本生成和合成资产负债图片生成通过，包括 Summer 的预览与持久化流程。引导式文本和图片响应格式探测也通过，精确活动模型保持不变。另行观察到了内存压力和 OEM 回收导致的失败；夹具通过不能证明任意图片的准确性或所有设备上的后台稳定性。未使用模型或后端回退。

后台验证检查通知对话框可见时不会启动引擎、权限状态与设置入口可用，以及后台空闲后的首个图片请求仍使用同一服务进程。原始日志、设备标识、硬件规格及设备专属内存测量应保留在仓库之外。

项目 host/Android 静态检查、Tauri 打包与测试、Android 生命周期和清单单元测试，以及 Summer 构建、单元测试和 Lint 检查已通过。可选的完整 LLMD Android Lint 仍报告生成的 TV manifest 问题 `MissingTvBanner` 和 `ImpliedTouchscreenHardware`；改动代码未发现新的 Lint 错误。

## CLI 与 OpenAI API 集成测试

`cargo test --workspace --locked` 执行 `cli/tests/cli.rs` 中针对真实 CLI 二进制的集成测试。每个用例使用一个 supervisor，由其创建并独占管理 CLI 进程、空模型目录和日志。运行器持有回环控制连接，接收有类型的启动、CLI 退出和清理完成事件。测试作用域成功必须收到清理成功确认，并核实 supervisor 的退出状态。命令与就绪等待均限时 30 秒；HTTP 调用也设置连接和请求超时。服务器用例绑定回环端口零，再从启动日志读取分配的端口。

正常结束作用域时半关闭连接，让运行器仍能接收清理结果。panic 或运行器终止会关闭连接，触发相同的清理责任：停止并回收 CLI，再删除其文件。`Drop` 提供后备清理，但不能将清理失败变为作用域通过。supervisor 使用独立终端进程组，避免 Ctrl-C 中断清理。强制终止 supervisor 可能阻止清理，但仍存活的运行器会检测到缺失的完成事件并失败。回归覆盖运行器取消、supervisor 终止和 Windows 文件删除失败。两个忽略的内部入口运行监督与取消夹具，不是真实模型检查。仅运行这些用例可使用 `cargo test -p llmd --locked --test cli`，或轻量兼容入口 `scripts/test-cli.sh`。不需要 curl、jq 或 Python。

`cli/tests/openai_api.rs` 中的真实模型检查默认忽略，普通 CI 会报告 ignored，不下载或调用模型。先启动 host 端点并导入模型，再明确运行：

```bash
LLMD_TEST_MODEL=<imported-model-id> LLMD_OPENAI_BASE_URL=http://127.0.0.1:11435 \
  cargo test -p llmd --locked --test openai_api -- --ignored
```

PowerShell 用户可先设置 `$env:LLMD_TEST_MODEL` 和 `$env:LLMD_OPENAI_BASE_URL`，再执行同一 Cargo 命令。`scripts/test-openai-api.sh` 是轻量兼容启动器；显式启用这些测试却未提供非空模型 ID 时会失败，不会报告通过。独立用例检查健康状态、模型清单、非空的非流式文本，以及严格生成恰好为 `{"value":"ok"}` 的 JSON（包括字符串枚举且无额外属性）。`LLMD_TEST_PROMPT` 可选地设置普通文本提示词。外部端点测试遵循环境代理设置，包括 `HTTP_PROXY`、`HTTPS_PROXY` 和 `NO_PROXY`；只有隔离的回环 CLI 夹具请求禁用代理。无需模型的回归通过全新子进程中的本地模拟代理验证此行为。每个请求超时为 180 秒。Rust 夹具在测试可执行文件内部串行执行生成请求，在发起每个 HTTP 请求前取得许可，避免本地排队时间消耗该超时。这同时适用于直接 Cargo 运行和 Shell 启动器，并保留独立的生成用例。无需模型的并发回归检查 HTTP 夹具的在途请求数量。该许可不协调其他测试进程或外部调用方。这些检查不覆盖流式响应、工具、多模态请求或 Android Binder IPC，也不启动或停止外部管理的端点。Android Appium 和桌面安装器测试保留现有的平台集成入口。
