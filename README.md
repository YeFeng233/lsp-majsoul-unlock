# 雀魂 Max Hook

雀魂 Max Hook 是面向安卓版雀魂的 LSPosed 模块，仓库对应的应用包名为 `com.yefeng.majmax.hookprobe`。它在游戏进程中处理雀魂的 WebSocket 消息，提供游戏内 MOD 设置；同一个 APK 也包含独立的管理界面、本地牌局助手和可收纳的半透明悬浮窗。项目不会替换游戏 APK，也不需要安装代理证书。

当前正式版本为 **0.6.0**，仅构建 **ARM64** APK。安装包和更新元数据发布在 [GitHub Releases](https://github.com/YeFeng233/lsp-majsoul-unlock/releases/latest)。

## 安装与使用

设备需要 Android 10 或更新版本、支持现代 Xposed API 102 的 LSPosed 环境，以及安卓版雀魂 `com.soulgamechst.majsoul`。从 Release 下载 `MajsoulMax-Hook-0.6.0-arm64-v8a.apk` 安装后，在 LSPosed 中启用模块并勾选雀魂作用域，然后完全重启游戏。同一签名的后续版本可覆盖安装；更新模块后仍需重启游戏才能载入新的 Hook。

游戏内“设置 → MOD设置”提供总开关、提示、表情、服务器信息、昵称等选项。独立应用的“概览”显示模块和版本信息；“日志”可切换 Hook 日志与助手日志，并通过系统分享面板导出故障 ZIP；“更新”会在启动时检查一次正式 Release，也能手动检查。更新页提供发布页面入口，安装仍由用户完成。右上角的信息按钮显示版本、构建提交、开发者和项目地址。

在“助手”页授权悬浮窗后，可以启动实时牌局助手。悬浮窗可拖动，支持调整宽度和透明度，也可以收纳为小浮标。助手使用随 APK 提供的 Akagi 轻量模型在手机本地计算推荐动作，并显示向听、进张、符合条件时的和牌率估计及风险指数。它只给出建议，不会代替玩家操作。风险指数不是经过校准的放铳概率；未公开的手牌和未来牌山也不在分析数据中。中途启动或连接中断后，助手可能需要重新进入牌局才能取得完整状态。

助手页允许分别为四麻和三麻导入兼容的单文件 `.onnx` 策略模型，单文件上限为 128 MiB。自定义模型只替换动作策略，其他分析指标仍使用内置算法。导入时会验证输入输出、元数据和样例推理；不兼容时继续使用内置模型。**把 PyTorch 检查点改名为 `.onnx` 不会完成转换**，包含 `data.pkl` 的 PyTorch ZIP 文件不能直接导入。模型协议和示例导出命令见 [ONNX 模型说明](tools/README_POLICY_ONNX.md)。

## 隐私与适用范围

牌局消息通过游戏进程中的 Hook 复制，经本机连接送到管理应用分析。正常推理不依赖云端服务；应用联网主要用于检查 GitHub Release。故障包由用户主动导出并选择分享对象，不会自动上传。日志不应包含原始牌局帧或账号凭据。

当前主要针对普通四麻、三麻和已验证的游戏协议版本。游戏更新、特殊玩法或不完整的牌局同步都可能使建议暂时不可用；请以游戏内状态为准。使用模块可能受到游戏规则或服务条款限制。

## 从源码构建

仓库包含 Android/Kotlin 界面、C++ Hook、Rust Modder 与 Akagi 本地分析组件，以及必要的上游源码和模型。GitHub Actions 在 `main` 更新时运行构建，在 `v*` 标签发布时生成正式 Release，附带 APK 和供应用检查更新的 `hook-update.json`。工作流运行 Rust 测试，并检查 APK 签名和 16 KiB 原生库对齐。签名密钥保存在该仓库的加密 Actions Secret 中，不在源码中。

本地构建需要 JDK 17、Android SDK Platform 35、Build Tools 35.0.0、NDK 27.2.12479018、Rust 的 `aarch64-linux-android` 目标和 `protoc`。在仓库根目录执行：

```powershell
rustup target add aarch64-linux-android
.\gradlew.bat -p .\hook-probe --no-daemon clean
.\hook-probe\build-native.ps1 -NdkPath '<Android SDK>\ndk\27.2.12479018'
.\gradlew.bat -p .\hook-probe --no-daemon assembleDebug
```

产物位于 `hook-probe/build/outputs/apk/debug/MajsoulHookProbe-debug.apk`。本地构建要使用与已安装版本相同的签名才能覆盖安装。

## 来源与许可

MOD 消息处理代码来自 [MajsoulMax-rs](https://github.com/Xerxes-2/MajsoulMax-rs)，牌局状态、分析和内置模型来自 [Akagi](https://github.com/shinkuan/Akagi)。本仓库保留了构建需要的上游文件及其许可证；具体来源见 [MajsoulMax-rs 来源说明](external/MajsoulMax-rs/UPSTREAM.md) 和 [Akagi 来源说明](external/Akagi/UPSTREAM.md)。Akagi 内置模型不是 Mortal 模型。
