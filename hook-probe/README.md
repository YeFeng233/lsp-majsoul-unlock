# 雀魂 Max Hook

这是面向 `com.soulgamechst.majsoul` 的进程内 MajsoulMax 实现。模块用现代 Xposed API 102 进入游戏进程，以 LSPosed Native Hook 拦截 IL2CPP 中的 BestHTTP WebSocket 二进制收发入口，再调用从固定上游版本编译的 Rust `Modder` 处理 Liqi 消息。它不启动本地代理、不安装 MITM 证书，也不改写游戏 APK。

## 独立管理应用原型

`hook-probe` 现在同时包含一个独立的管理应用入口。桌面启动后可在“概览、日志、更新”三个页面之间切换，并使用同一份雀魂 Max 图标。管理应用运行在自己的进程，不能把它的本地日志当作游戏进程已生效的证据；游戏内 MOD 选项仍在“设置 → MOD设置”中调整。

更新页使用构建时的 `updateOwner`、`updateRepo` 和 `updateChannel` Gradle 属性生成 `UPDATE_OWNER`、`UPDATE_REPO` 和 `UPDATE_CHANNEL`。默认更新源已配置为 `YeFeng233/lsp-majsoul-unlock` 的 GitHub Releases，应用启动时自动检查一次，更新页也提供手动检查按钮。正式发布使用 `v*` 标签，并随 APK 发布 `hook-update.json` 元数据；检查只接受非草稿、非预发布的稳定 Release。独立应用的界面与发布计划见 [`docs/HOOK_MANAGER_UI_DESIGN.md`](../docs/HOOK_MANAGER_UI_DESIGN.md)。

本阶段已完成管理应用外壳、图标资源、主题、导航、概览状态空态、管理侧日志搜索与级别筛选，以及 GitHub Releases 手动和启动自动更新检查。游戏侧结构化诊断文件、root 只读适配器仍按设计文档的后续阶段实现；不会为了让页面显示绿色状态而伪造游戏进程事件。

当前实现已在 Android 17 ARM64、LSPosed IT 2.2.0-it (7888)、游戏 `4.0.16_MC` / 222 上实测。登录到大厅时已观察到双向字节数组替换，游戏公告中显示 `雀魂Max-rs载入成功` / `0.7.0`。房间、观战和完整对局流程尚未覆盖。

## 游戏内 MOD 设置页

模块会在游戏 `UI.Settings.UI_Settings` 的设置窗口中追加第六个“MOD设置／MOD設置”标签。页面使用游戏自己的 TabView、FormGroupBase、FormItemSwitcher、滚动区和字体资源，Native 侧通过 `LuaClient.OnLoadFinished` 与 `LuaLooper.Update` 在 Unity 主线程执行 Lua 页面脚本。当前页面包含总开关、强制提示、解锁表情、服务器信息、昵称审查、随机主角色六个开关，以及一个自定义昵称行；点击昵称行会打开游戏“设置 → 其他 → 礼品码”使用的同款原生弹窗，弹窗内同时提供确认和“恢复原昵称”按钮。设置通过 Rust C ABI 合并写入 `settings.mod.json`，不会发起真实改名请求。

已在目标设备 `10.10.1.20:40747` 的 `4.0.16_MC` 设置窗口取得第六个标签和页面截图，并点击“解锁表情”确认文件中的 `emojiSwitch` 从 `false` 写为 `true`；昵称行已打开游戏礼品码同款弹窗并显示 Android 输入法。随机主角色候选选择器和场景重建回归仍属于后续适配工作。

0.4.3 修复昵称确认键：等原生弹窗 `OnShow` 初始化完成后注入，去掉 uGUI InputField 不支持的 `onSelect` 访问，并通过原生 `Confirm` 入口保存配置。“恢复原昵称”与确认键在同一弹窗中并排显示，不再单独占一行。真机已验证输入后确认保存、重新打开和重启后回填、恢复为空字符串，以及返回普通礼品码弹窗时恢复单确认键和原文案。已显示的玩家昵称仍需对应游戏数据刷新；本次测试未发起真实改名或礼品码兑换。

## 结构

- `ProbeModule.java`：API 102 Java 入口，在 `MainActivity.onCreate` 前准备配置并初始化 Rust。资源直接从 `getModuleApplicationInfo().sourceDir` 指向的模块 APK 读取，不依赖目标应用的包可见性。
- `probe.cpp`：只 Hook `BestHTTP.WebSocket.WebSocket.Send(byte[])`、接收二进制回调以及连接关闭入口；使用 IL2CPP API 分配新的托管 `byte[]`，并在接收回调线程投递本地通知。
- `rust-modder/src/lib.rs`：同步 C ABI、按连接保存 Liqi 请求编号到 RPC 名称的映射，并将处理结果表示为放行、替换、丢弃和注入。
- `rust-modder/src/settings.rs`：上游 `Modder` 所需的最小设置层；代理、TLS、证书和 HTTP 转发代码没有进入游戏进程。
- `external/MajsoulMax-rs/src/modder.rs`：直接编译固定 gitlink 提交 `7065716d12514b0a6a4bbc55adf29c0b5b2bacaa` 的上游源码，而不是维护一份分叉副本。

处理路径如下：

```text
游戏逻辑 -> BestHTTP Send(byte[]) -> Rust Modder -> 原 WebSocket/TLS
游戏逻辑 <- 原接收回调 <- Rust Modder <- BestHTTP 二进制回调
                              ^
                              └─ 本地 NotifyAccountUpdate 注入队列
```

任一初始化、签名定位、Rust 调用或托管数组分配失败时，消息保持原样放行。日志只记录方向、动作和字节长度，不输出 Liqi 正文、凭据或 URL。

## 构建

需要 JDK 17 或更高版本、Android SDK Platform 35、Build Tools 35.0.0、Android NDK，以及支持 Rust 2024 edition 的 Rust 工具链。为 Rust 安装 Android ARM64 目标：

```powershell
rustup target add aarch64-linux-android
```

在仓库根目录执行。`clean` 会删除 `hook-probe/build/native-libs`，因此顺序必须是 clean、原生构建、APK 构建：

```powershell
.\gradlew.bat -p .\hook-probe --no-daemon clean
.\hook-probe\build-native.ps1 -NdkPath 'C:\Android\Sdk\ndk\27.2.12479018'
.\gradlew.bat -p .\hook-probe --no-daemon assembleDebug
```

`build-native.ps1` 先为 `aarch64-linux-android` 编译 `libmajsoulmodder.so`，再链接 `libmajsoulprobe.so`。两份 ELF 的 LOAD 段按 16 KiB 对齐；Gradle 将它们不压缩地放入 APK。产物为：

```text
hook-probe/build/outputs/apk/debug/MajsoulHookProbe-debug.apk
```

可用以下命令检查 APK 内原生库的 16 KiB ZIP 对齐：

```powershell
zipalign -c -P 16 4 .\hook-probe\build\outputs\apk\debug\MajsoulHookProbe-debug.apk
```

## 安装与验证

```powershell
adb connect 10.10.1.20:40747
adb -s 10.10.1.20:40747 install -r .\hook-probe\build\outputs\apk\debug\MajsoulHookProbe-debug.apk
adb -s 10.10.1.20:40747 shell am force-stop com.soulgamechst.majsoul
adb -s 10.10.1.20:40747 shell am start -W -n com.soulgamechst.majsoul/com.soulgamechst.mahjongsoulsdk.MainActivity
adb -s 10.10.1.20:40747 logcat -s MajsoulHook:I
```

模块包名为 `com.yefeng.majmax.hookprobe`，名称为“雀魂 Max Hook”，静态作用域只有游戏包。首次安装后需在 LSPosed 中启用一次；同包名更新会保留现有启用状态和作用域。Java 入口日志写入 LSPosed 模块日志，原生与 Rust 日志使用 `MajsoulHook` 标签。

成功启动应依次出现：

```text
Rust Modder initialized
HOOK BestHTTP.WebSocket.WebSocket.Send(byte[]) result=0
HOOK BestHTTP.WebSocket.WebSocket binary callback result=0
BestHTTP Modder ready; array header=32
IN message replaced: ... -> ... bytes
```

配置位于游戏私有目录：

```text
/data/user/0/com.soulgamechst.majsoul/files/majsoulmax-hook/
```

模块升级会更新 `max_data.yaml`，但保留现有 `settings.mod.json`。如果设备上原有 `com.yefeng.majmax` 应用已经保存了设置，可在 root 设备上同步后重启游戏：

```powershell
.\hook-probe\sync-device-settings.ps1 `
    -AdbPath 'C:\Android\Sdk\platform-tools\adb.exe' `
    -Serial '10.10.1.20:40747' `
    -RestartGame
```

停止使用时，在 LSPosed 关闭模块并重启游戏即可。更完整的版本分析和真机证据见 `docs/HOOK_FEASIBILITY_API102.md`。
