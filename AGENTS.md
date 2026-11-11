# AGENTS.md

## 静态检查

交付代码改动前，在仓库根目录运行相关静态检查：

```bash
./scripts/check-static.sh --host
```

涉及 Android、JNI、Tauri 移动端生成或 Android 工作流时，运行 Android 检查：

```bash
./scripts/check-static.sh --android
```

如果仅修改 Android 生成的 Gradle、Kotlin 或 AIDL 部分，可以使用范围更小的检查：

```bash
./scripts/check-static.sh --android-kotlin
```

前端检查要求先安装依赖：

```bash
npm --prefix app ci
```

交付前检查补丁格式：

```bash
git diff --check
```

即使改动有意包含 `app/src-tauri/gen/android` 下生成的 Android 源码和资源，它们仍可能被 `.gitignore` 忽略。添加这类文件时，使用以下命令核实：

```bash
git status --short --untracked-files=all
```

只有确实需要纳入改动的具体生成文件，才使用 `git add -f <path>`。

## 测试

服务器或 API 行为变化时，运行聚焦的 Rust 测试：

```bash
cargo test -p llmd-server
```

共享 crate、CLI 行为或跨 crate 约定变化时，运行整个工作区的 Rust 测试：

```bash
cargo test --workspace
```

`app/src-tauri` 行为变化时，运行 Tauri Rust 测试：

```bash
cargo test --manifest-path app/src-tauri/Cargo.toml
```

OpenAI 兼容 HTTP API 变化时，针对运行中的端点测试：

```bash
LLMD_TEST_MODEL=<imported-model-id> LLMD_OPENAI_BASE_URL=http://127.0.0.1:11435 cargo test -p llmd --locked --test openai_api -- --ignored
```

验证 Android 模型导入时，针对已连接设备运行 Appium 流程：

```bash
GEMMA_MODEL_PATH=/path/to/chosen-model.litertlm ANDROID_UDID=<device-serial> scripts/test-android-appium.sh
```

Android IPC 授权变化时，在设备或模拟器上手动验证：

- 未授权调用方收到 `authorization_required` IPC 响应；
- 可通过 `com.storytellerf.llmd.action.AUTHORIZE_CALLER` 打开授权 Activity；
- 批准调用方后，授权通过 DataStore 持久化；
- 批准后，同一调用方可调用健康检查、模型列表和聊天 IPC 方法；
- IPC 授权路径不引入 `SharedPreferences` 或 `runBlocking`。

Android 不提供设备本地的 OpenAI HTTP 服务器。UI 使用应用内原生桥接，其他 Android 应用通过获授权的 Binder IPC 服务访问。
