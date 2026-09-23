# 雀魂 Max Hook

雀魂 Max Hook 是面向安卓版雀魂的 LSPosed 模块，仓库对应的应用包名为 `com.yefeng.majmax.hookprobe`。它在游戏进程中处理雀魂的 WebSocket 消息，提供游戏内 MOD 设置；同一个 APK 也包含独立的管理界面、本地牌局助手和可收纳的半透明悬浮窗。支持 LSPosed 加载及实验性的 LSPatch 嵌入打包，不需要安装代理证书。

当前正式版本为 **0.7.1**，提供 **ARM64 版**和 **Universal 通用版**。安装包和更新元数据发布在 [GitHub Releases](https://github.com/YeFeng233/lsp-majsoul-unlock/releases/latest)，本次改动见 [0.7.1 更新日志](release-notes/v0.7.1.md)。

| 版本 | 包含的 Android ABI | 适用环境 |
| --- | --- | --- |
| ARM64 | `arm64-v8a` | ARM64 手机，安装包较小 |
| Universal | `arm64-v8a`、`armeabi-v7a`、`x86`、`x86_64` | ARM 32/64 位及 x86 32/64 位设备、模拟器 |

通用版为每种 ABI 包含 Hook、Rust Modder、本地 AI、JNI 和 ONNX 运行库。最低系统版本仍为 Android 10；游戏客户端和注入框架也需支持设备实际使用的 ABI。

## 安装与使用

使用 LSPosed 时，设备需要 Android 10 或更新版本、支持现代 Xposed API 102 的 LSPosed 环境，以及安卓版雀魂 `com.soulgamechst.majsoul`。安装与设备匹配的 ARM64 或 Universal 模块 APK 后，在 LSPosed 中启用模块并勾选雀魂作用域，然后完全重启游戏。同一签名的后续版本可覆盖安装；更新模块后仍需重启游戏才能载入新的 Hook。

游戏内“设置 → MOD设置”提供总开关、提示、表情、服务器信息、昵称等选项。独立应用的“概览”显示模块和版本信息；“日志”可切换 Hook 日志与助手日志，并通过系统分享面板导出故障 ZIP；“更新”会在启动时检查一次正式 Release，也能手动检查。更新页提供发布页面入口，安装仍由用户完成。右上角的信息按钮显示版本、构建提交、开发者和项目地址。

0.7.1 起，开启 MOD 和“解锁表情”后，若服务器以 `2208` 拒绝当前本地角色的表情，模块会在本机补充显示。正常发送成功的表情仍使用游戏广播，不重复显示；断网、其他错误、房间禁用表情或关闭功能时不会补偿。角色和表情的本地解锁不会改变服务器上的拥有记录，补偿显示也不会发送给其他玩家。

在“助手”页授权悬浮窗后，可以启动实时牌局助手。悬浮窗可拖动，支持调整宽度和透明度，也可以收纳为小浮标。助手使用随 APK 提供的 Akagi 轻量模型在手机本地计算推荐动作，并显示向听、进张、符合条件时的和牌率估计及风险指数。它只给出建议，不会代替玩家操作。风险指数不是经过校准的放铳概率；未公开的手牌和未来牌山也不在分析数据中。中途启动或连接中断后，助手可能需要重新进入牌局才能取得完整状态。

助手页允许分别为四麻和三麻导入兼容的单文件 `.onnx` 策略模型，单文件上限为 128 MiB。自定义模型只替换动作策略，其他分析指标仍使用内置算法。导入时会验证输入输出、元数据和样例推理；不兼容时继续使用内置模型。**把 PyTorch 检查点改名为 `.onnx` 不会完成转换**，包含 `data.pkl` 的 PyTorch ZIP 文件不能直接导入。模型协议和示例导出命令见 [ONNX 模型说明](tools/README_POLICY_ONNX.md)。

### LSPatch（实验性兼容）

0.7.0 新增 Unity 启动路径兼容处理：LSPatch 将原始 APK 放在应用缓存目录，雀魂的 Unity 启动检查会拒绝该路径并退出。模块只在 Unity 的 `nativeRender` 查询 APK 路径时，提供指向同一原始 APK 的进程内文件描述符路径，保留原始 ZIP 的资源位置。

打包时使用原版游戏 APK，并嵌入包含修复的模块 APK。已验证的组合为 JingMatrix LSPatch 1.2、启用 `--injectdex`、签名兼容级别 `-l 2`。更新独立安装的模块不会更新游戏中嵌入的副本，需要重新打包游戏；不嵌入模块的对照包也不会包含此兼容处理。

初始化会等待 IL2CPP 和 ToLua 两个原生库都加载后，再开始检查元数据。首次下载资源或等待登录超过 30 秒时，不会提前耗尽初始化窗口。

已在 Android 17 ARM64 设备、雀魂 `4.0.16_MC` 上验证正常启动进入大厅、MOD 载入公告、游戏内“MOD设置”页面，以及收发消息 Hook。对局和助手连接尚未验证。

## 隐私与适用范围

牌局消息通过游戏进程中的 Hook 复制，经本机连接送到管理应用分析。正常推理不依赖云端服务；应用联网主要用于检查 GitHub Release。故障包由用户主动导出并选择分享对象，不会自动上传。日志不应包含原始牌局帧或账号凭据。

当前主要针对普通四麻、三麻和已验证的游戏协议版本。游戏更新、特殊玩法或不完整的牌局同步都可能使建议暂时不可用；请以游戏内状态为准。使用模块可能受到游戏规则或服务条款限制。

## 从源码构建

仓库包含 Android/Kotlin 界面、C++ Hook、Rust Modder 与 Akagi 本地分析组件，以及必要的上游源码和模型。GitHub Actions 在 `main` 更新时运行构建，在 `v*` 标签发布时生成正式 Release，附带 APK 和供应用检查更新的 `hook-update.json`。工作流运行 Rust 测试，并检查 APK 签名和 16 KiB 原生库对齐。签名密钥保存在该仓库的加密 Actions Secret 中，不在源码中。

本地构建需要 JDK 17、Android SDK Platform 35、Build Tools 35.0.0、NDK 27.2.12479018、Rust 和 `protoc`。原生构建脚本可在 Windows、Linux 和 macOS 的 PowerShell 中运行；先将 `cargo` 和 `protoc` 加入 PATH（也可通过 `PROTOC` 指定编译器）。在仓库根目录执行：

```powershell
rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android
.\hook-probe\build-native.ps1 -NdkPath '<Android SDK>\ndk\27.2.12479018' -Abis all
.\gradlew.bat -p .\hook-probe --no-daemon assembleDebug
```

`assembleDebug` 同时生成两个 APK：

- ARM64：`hook-probe/build/outputs/apk/arm64/debug/MajsoulHookProbe-arm64-debug.apk`
- 通用版：`hook-probe/build/outputs/apk/universal/debug/MajsoulHookProbe-universal-debug.apk`

只构建 ARM64 时，原生脚本使用 `-Abis arm64-v8a`，Gradle 使用 `assembleArm64Debug`；只打包通用版使用 `assembleUniversalDebug`。两种 APK 使用相同包名、版本号和签名，可相互覆盖安装。本地构建要使用与已安装版本相同的签名。

CI 会生成两种安装包，检查各 ABI 的运行库是否齐全、ELF 架构是否匹配、动态依赖是否完整、64 位原生库是否满足 16 KiB 页对齐，以及 APK 的签名与 ZIP 对齐。发布的 `hook-update.json` 同时列出两种产物，并以通用版作为默认资源。发布新版本时先同步应用版本号，并编写 `release-notes/<标签名>.md`；标签工作流会将该文件的自然语言更新说明写入 Release。

## 来源与许可

本项目采用 GNU GPL v3 开源，完整协议见根目录的 [LICENSE](LICENSE)。MOD 消息处理代码来自 [MajsoulMax-rs](https://github.com/Xerxes-2/MajsoulMax-rs)，牌局状态、分析和内置模型来自 [Akagi](https://github.com/shinkuan/Akagi)。这些第三方文件保留各自的原始许可与声明；具体来源见 [MajsoulMax-rs 来源说明](external/MajsoulMax-rs/UPSTREAM.md)、[Akagi 来源说明](external/Akagi/UPSTREAM.md) 以及 Akagi 的 [LICENSE.txt](external/Akagi/LICENSE.txt) 和 [NOTICE](external/Akagi/NOTICE)。Akagi 内置模型不是 Mortal 模型。
