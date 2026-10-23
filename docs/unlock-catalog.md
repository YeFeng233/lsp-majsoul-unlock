# 独立解锁清单

0.11.0 起，管理应用在每次启动时检查本仓库 `main/catalog/manifest.json`。版本较新时自动下载 `unlock-data.json`，验证长度、SHA-256、栏目、数字 ID 和最低应用版本，再完整保存到应用私有目录。网络或校验失败时保留原清单，首次离线启动使用 APK 内置清单。

“更新”页面显示本地版本、皮肤数量、上次检查时间和游戏载入状态，并提供“立即更新解锁清单”按钮。清单涵盖原有的角色、皮肤、称号、装扮、插画、表情和终局数据。更新清单不替换 `settings.mod.json`，因此不会重置角色和皮肤选择。

下载成功后，完全退出并重新启动雀魂。Hook 在初始化 Rust Modder 前，通过只允许雀魂 UID 调用的本机接口取得最新清单，在游戏私有目录原子替换 `max_data.yaml`。已开始的对局不会重建 Modder 或切换清单。新 APK 的界面资源升级也不会覆盖版本更高的清单。管理应用不可用时保留最后可用数据；首次使用嵌入模块时采用 APK 内置数据。LSPatch 用户需先将支持此功能的新模块重新嵌入游戏。

## 维护仓库中的版本

编辑 `external/MajsoulMax-rs/liqi_config/max_data.yaml` 中的已确认 ID，再从仓库根目录执行：

```powershell
python tools/update_unlock_catalog.py --version 3 --name 2026.10.24.1 --date 2026-10-24
python tools/update_unlock_catalog.py --check
```

版本号必须严格递增，日期和名称按实际发布时间填写。将源 YAML 和生成的两个 JSON 文件放在同一次提交中推送到 `main`。应用按数字 `version` 比较新旧，不按版本名称或日期比较。同版本但不同校验值会被拒绝，不会偷偷替换内容。更高本地版本不会被远端旧版本降级。

`manifest.json` 中的 `schemaVersion=1` 和 `minModuleVersionCode=25` 表示当前协议及最低支持版本。文件名固定，远端数据不能指定其他路径、下载脚本或修改 MOD 设置。单份数据最大 512 KiB，版本文件最大 16 KiB。清单本身只包含七个固定栏目的无符号 32 位数字 ID。

CI 会核对源 YAML、JSON、大小和校验值一致。只修改清单、源 YAML、生成工具或本说明时，不触发 APK 重编译；清单相关文件由独立校验工作流检查。后续应用版本仍会将当时的清单打包，供离线首次启动使用。

## GitHub 加速

默认直连。在“更新 → GitHub 仓库加速”中启用开关，填写自有或已选服务的 HTTPS 地址前缀并保存。例如前缀为 `https://accelerator.example/` 时，请求形式为 `https://accelerator.example/https://raw.githubusercontent.com/YeFeng233/lsp-majsoul-unlock/main/catalog/manifest.json`。此地址仅为格式示例，不是应用预设的公共服务。

设置用于清单、GitHub Release 版本检查、更新元数据及安装包下载。应用内网络请求失败时会尝试原始直连地址；安装包交由浏览器下载，加速服务不可用时可关闭加速后重试。发布说明页面始终直接打开 GitHub。

只接受不带账号、查询参数或片段的 HTTPS 前缀。请求不携带 GitHub 登录信息或令牌。清单字段和 SHA-256 校验始终执行，不因使用加速而关闭；校验失败不会替换本地清单。
