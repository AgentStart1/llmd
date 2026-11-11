# llmd

`llmd` 通过 OpenAI 兼容 API 提供本地 LiteRT-LM 模型服务。

## 平台策略

- 桌面端：Tauri v2 应用，通过 `litertlm-rs` 绑定调用原生 LiteRT-LM C API。
- 终端：Rust CLI/TUI，使用相同的原生绑定。
- Android：Tauri 移动端 UI，使用原生 LiteRT-LM Android 推理。Android 使用官方 Kotlin SDK，不使用桌面端的 C API 绑定。

没有默认模型，也不会回退到其他模型。模型列表只包含已导入本地、非空的 `.litertlm` 文件。请求必须给出精确模型 ID（不含扩展名的文件名）；缺失或未知 ID 会失败，不会改选其他模型。

## 工作区

```text
app                    Tauri desktop and mobile shell
cli                    Terminal CLI/TUI
crates/llmd-core       Shared API types and provider trait
crates/llmd-rlitert    Desktop/terminal provider backed by litertlm-rs
crates/llmd-server     OpenAI-compatible HTTP API
docs                   Architecture and test notes
```

## CLI

启动本地 API 服务器：

```bash
cargo run -p llmd -- serve --port 11435
```

列出已下载到本地的 LiteRT-LM 模型：

```bash
cargo run -p llmd -- models
```

执行一次提示词请求：

```bash
cargo run -p llmd -- chat --model <imported-model-id> "Hello"
```

打开终端 UI：

```bash
cargo run -p llmd -- tui
```

## HTTP API

```bash
curl http://127.0.0.1:11435/health
```

```bash
curl http://127.0.0.1:11435/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gemma-4-E2B-it",
    "messages": [{"role": "user", "content": "Hello"}]
  }'
```

`response_format` 接受 OpenAI Chat Completions 的 `json_object` 和 `json_schema` 对象。桌面端和 Android 提供方将 schema 传给 LiteRT-LM 原生约束解码器。`json_object` 使用对象 schema；`json_schema` 传入用户提供的 schema。token 限制仍可能截断输出。不支持的 schema 会产生后端错误。

```json
{
  "response_format": {"type": "json_object"}
}
```

## 桌面端

```bash
cd app
npm install
npm run tauri dev
```

桌面应用包含 Status、Models 和 Logs 页面。在 Models 输入本地 `.litertlm` 文件路径即可导入，也可明确输入 `gemma-4-E2B-it` 下载该模型（支持 `HF_TOKEN`）。文件保存在操作系统本地数据目录的 `llmd/models` 下。设置 `LLMD_MODEL_DIR` 可改用其他目录，包括已有模型目录。旧 rlitert-lm 下载文件不会自动移动或删除，可按文件路径导入。模型 ID 是去掉 `.litertlm` 的文件名；导入绝不覆盖已有模型。

桌面构建需要 libclang；不在默认搜索路径时设置 `LIBCLANG_PATH`。固定版本的 `litertlm-rs`/`litertlm-sys` 0.16.3 会下载经过校验和验证的 LiteRT-LM 0.16.0 原生库，并复制到 Cargo 可执行文件旁。分发应用包时也须包含原生库；平台专用 Tauri 配置会为 Windows、Linux 和 Apple Silicon macOS 打包。Windows 要求 MSVC target；此绑定没有 Intel macOS 预构建包。Android 继续使用官方 Kotlin SDK。目前推理使用 CPU，`--pool-size` 限制并发请求数。

## Android

Android 使用 `app` 中共享的 Tauri UI。设备应用应提供与桌面和 CLI 相同的 OpenAI 兼容 API，由原生 `litertlm-android` 支持。

在 Models 页面选择本地模型文件导入。选择已导入模型会打开详情页，可从应用私有存储删除文件。导入、打开应用、检查健康状态及绑定 IPC 都不会加载模型。在 Status 选择已导入模型并开启 **Keep model service on**，等到 **Ready** 再调用。前台服务在客户端断开后仍保持该精确模型已加载；关闭开关会取消请求并释放模型。更换模型或模型文件前先停止服务。进程终止后状态回到 Off，不会静默重启模型。

Android 13 及以上版本开启服务时，请允许通知权限，使持续状态和停止操作可见。Status 会显示通知是否获准。若设备限制后台应用，打开 **System background settings**，允许 LLMD 后台运行。部分厂商还要求在最近任务中打开 LLMD 卡片菜单，选择 **Lock app** 或等效保护选项。这些系统设置与模型服务开关相互独立。

下载明确选定的模型，例如具备完整视觉能力的 Gemma 4 E4B 文件：

```bash
GEMMA_REPO_ID=litert-community/gemma-4-E4B-it-litert-lm \
GEMMA_MODEL_FILE=gemma-4-E4B-it.litertlm scripts/download-gemma-model.sh
```

为可调试 APK，将模型推送到已连接的 Android 设备：

```bash
GEMMA_MODEL_PATH=/path/to/chosen-model.litertlm \
ANDROID_UDID=<device-serial> scripts/prepare-android-model.sh
```

模型文件已被 git 忽略。Android 准备脚本要求设置 `GEMMA_MODEL_PATH`，并在应用私有的 `files/models` 目录保留原文件名。默认目标是 debug 包 `com.storytellerf.llmd.debug`；准备其他可调试变体时设置 `ANDROID_PACKAGE`。

通过 Appium 运行 Android 端到端测试。默认构建并安装普通 `debug` APK（包名 `com.storytellerf.llmd.debug`），将模型推送到设备 Downloads，通过 Android 文档选择器导入，再选择模型、开启服务，并通过 Binder IPC 检查：

```bash
GEMMA_MODEL_PATH=/path/to/chosen-model.litertlm \
ANDROID_UDID=<device-serial> scripts/test-android-appium.sh
```

调查混淆问题时，使用 `e2e` buildType。它保留 release 压缩，同时使用 debug 签名供本地安装：

```bash
GEMMA_MODEL_PATH=/path/to/chosen-model.litertlm \
ANDROID_UDID=<device-serial> scripts/test-android-appium.sh --e2e
```

Android 只通过获授权的 Binder IPC 接口向其他 Android 应用提供 LiteRT-LM，不监听设备 TCP 端口。见 `docs/android-tauri-ipc-api.md`。

## 测试

生命周期管理、生成的 Android 打包、验证命令和真机限制见 [DEVELOPMENT.md](DEVELOPMENT.md)。

工作区、CLI 和按需启用的真实模型 API 验证见[开发文档](DEVELOPMENT.md#cli-与-openai-api-集成测试)。

运行时与模型已安装时，手动运行真实 LiteRT-LM 冒烟测试：

```bash
cargo test -p llmd-rlitert real_rlitert_chat_smoke_test -- --ignored
```
