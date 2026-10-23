# 独立解锁清单

0.11.0 起，管理应用在每次启动时检查本仓库 `main/catalog/manifest.json`。版本较新时自动下载 `unlock-data.json`，验证长度、SHA-256、栏目、数字 ID 和最低应用版本，再完整保存到应用私有目录。网络或校验失败时保留原清单，首次离线启动使用 APK 内置清单。

“更新”页面显示本地版本、皮肤数量、上次检查时间和游戏载入状态，并提供“立即更新解锁清单”按钮。清单涵盖原有的角色、皮肤、称号、装扮、插画、表情和终局数据。更新清单不替换 `settings.mod.json`，因此不会重置角色和皮肤选择。

下载成功后，完全退出并重新启动雀魂。Hook 在初始化 Rust Modder 前，通过只允许雀魂 UID 调用的本机接口取得最新清单，在游戏私有目录原子替换 `max_data.yaml`。已开始的对局不会重建 Modder 或切换清单。新 APK 的界面资源升级也不会覆盖版本更高的清单。管理应用不可用时保留最后可用数据；首次使用嵌入模块时采用 APK 内置数据。LSPatch 用户需先将支持此功能的新模块重新嵌入游戏。

## 维护仓库中的版本

编辑 `external/MajsoulMax-rs/liqi_config/max_data.yaml` 中的已确认 ID，再从仓库根目录执行：

```powershell
python tools/update_unlock_catalog.py --version 4 --name 2026.10.24.1 --date 2026-10-24
python tools/update_unlock_catalog.py --check
```

版本号必须严格递增，日期和名称按实际发布时间填写。将源 YAML 和生成的两个 JSON 文件放在同一次提交中推送到 `main`。应用按数字 `version` 比较新旧，不按版本名称或日期比较。同版本但不同校验值会被拒绝，不会偷偷替换内容。更高本地版本不会被远端旧版本降级。

`manifest.json` 中的 `schemaVersion=1` 和 `minModuleVersionCode=25` 表示当前协议及最低支持版本。文件名固定，远端数据不能指定其他路径、下载脚本或修改 MOD 设置。单份数据最大 512 KiB，版本文件最大 16 KiB。清单本身只包含七个固定栏目的无符号 32 位数字 ID。

CI 会核对源 YAML、JSON、大小和校验值一致。只修改清单、源 YAML、生成工具或本说明时，不触发 APK 重编译；清单相关文件由独立校验工作流检查。后续应用版本仍会将当时的清单打包，供离线首次启动使用。

## GitHub 加速

默认直连。在“更新 → GitHub 仓库加速”中启用开关，从“加速源”选择公共线路并保存。提供五个预设：

- [GH-Proxy](https://gh-proxy.com/docs/github-accelerator)：`https://gh-proxy.com/`
- [LLKK](https://gh.llkk.cc/)：`https://gh.llkk.cc/`
- [GHFast](https://ghfast.top/)：`https://ghfast.top/`
- [GH-Proxy 备用域名](https://gh-proxy.com/docs/quick-start)：`https://gh-proxy.org/`（与 GH-Proxy 同服务的另一个访问域名）
- [iSteed](https://cors.isteed.cc/)：`https://cors.isteed.cc/`

上述地址在 2026-10-23 检查时均能返回本项目 JSON 清单；公共服务的网络可用性可能变化，可切换其他线路或直连。加速清单版本检查会带刷新参数，减少旧版本缓存造成的等待。

“自定义地址（备选）”保留手动填写功能，切换公共线路不会清空已保存的自定义地址。填写 HTTPS 地址前缀并保存，例如前缀为 `https://accelerator.example/` 时，请求形式为 `https://accelerator.example/https://raw.githubusercontent.com/YeFeng233/lsp-majsoul-unlock/main/catalog/manifest.json`。此地址仅为格式示例。

点击“测速并自动选择”，会测试所有预设及有效的自定义地址，以手机当前网络实际完成项目清单请求的耗时（毫秒）比较快慢。计时包括连接、HTTPS、下载及清单格式检查，显示的是文件请求耗时，不是 ICMP ping 或安装包下载速度。单条线路在测试时不使用直连回退，避免把回退后的直连误记为该线路的成绩。返回网页、过旧版本、同版本但校验值不一致或请求失败的线路不会参与选择。

测试完成后自动启用并保存成功线路中耗时最低的一条，后续检查和下载使用该设置。若全部失败则保留原设置，自定义地址始终保留。界面显示每条线路的耗时或失败原因；切换页面或旋转不会重复发起测试。两次手动测试至少间隔 30 秒，最多同时请求 3 条线路，避免频繁请求公共服务。结果保留在本次应用进程中，选中的连接设置会持久保存。测速使用清单文件，部分公共线路可能限制 GitHub API，正式版本检查仍会按原逻辑回退直连。

设置用于清单、GitHub Release 版本检查、更新元数据及安装包下载。应用内网络请求失败时会尝试原始直连地址；安装包交由浏览器下载，加速服务不可用时可关闭加速后重试。发布说明页面始终直接打开 GitHub。

只接受不带账号、查询参数或片段的 HTTPS 前缀。请求不携带 GitHub 登录信息或令牌。清单字段和 SHA-256 校验始终执行，不因使用加速而关闭；校验失败不会替换本地清单。
