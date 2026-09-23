package com.yefeng.majmax.hookprobe.manager

import android.content.Context
import android.content.Intent
import android.content.ClipData
import android.content.ClipboardManager
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.Image
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Article
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.Info
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.SystemUpdate
import androidx.compose.material.icons.filled.SmartToy
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material.icons.filled.Share
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.yefeng.majmax.hookprobe.BuildConfig
import com.yefeng.majmax.hookprobe.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL

private val Green = Color(0xFF007A46)
private val Cream = Color(0xFFF7F7EF)

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        DiagnosticsAccess.grantToGame(this)
        DiagnosticsStore.cleanExpiredExports(this)
        setContent { HookManagerApp() }
    }
}

private enum class Destination(val title: String) {
    Overview("概览"), Assistant("助手"), Logs("日志"), Updates("更新")
}

private data class ModuleStatus(
    val diagnosticAvailable: Boolean,
    val lastEvent: String?,
)

private sealed interface UpdateState {
    data object NotConfigured : UpdateState
    data object Idle : UpdateState
    data object Checking : UpdateState
    data object UpToDate : UpdateState
    data class Available(val versionCode: Int, val versionName: String, val notes: String, val pageUrl: String) : UpdateState
    data class Error(val message: String) : UpdateState
}

private object ManagerDiagnostics {
    fun append(context: Context, level: String, source: String, message: String) {
        val code = when {
            source == "diagnostics" -> "EXPORT_FAILED"
            message.contains("启动目标游戏") -> "GAME_LAUNCH_REQUESTED"
            message.contains("刷新诊断状态") -> "DIAGNOSTICS_REFRESH"
            message.contains("设置仍由游戏内页面管理") -> "GAME_SETTINGS_OPENED"
            else -> "MANAGER_ACTION"
        }
        DiagnosticsStore.appendAssistant(context, level, "manager", code,
            JSONObject().put("reason", source.take(24)))
    }

    fun read(context: Context): List<DiagnosticRecord> = DiagnosticsStore.read(context, DiagnosticKind.Assistant)
}

@Composable
@OptIn(ExperimentalMaterial3Api::class)
private fun HookManagerApp() {
    var selectedIndex by rememberSaveable { mutableIntStateOf(0) }
    var showAbout by rememberSaveable { mutableStateOf(false) }
    val destination = Destination.entries[selectedIndex]
    val configured = BuildConfig.UPDATE_OWNER.isNotBlank() && BuildConfig.UPDATE_REPO.isNotBlank()
    var updateState by remember(configured) {
        mutableStateOf<UpdateState>(if (configured) UpdateState.Checking else UpdateState.NotConfigured)
    }
    val updateScope = rememberCoroutineScope()

    LaunchedEffect(configured) {
        if (configured) {
            updateState = UpdateState.Checking
            updateState = withContext(Dispatchers.IO) { checkForUpdate() }
        }
    }

    HookManagerTheme {
        Scaffold(
            topBar = {
                TopAppBar(
                    title = {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Image(
                                painter = painterResource(R.drawable.app_icon),
                                contentDescription = null,
                                modifier = Modifier.size(38.dp),
                            )
                            Spacer(Modifier.width(10.dp))
                            Column {
                                Text("雀魂 Max Hook", fontWeight = FontWeight.SemiBold)
                                Text("独立模块管理", fontSize = 11.sp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                        }
                    },
                    actions = {
                        IconButton(onClick = { showAbout = true }) {
                            Icon(Icons.Default.Info, contentDescription = "关于")
                        }
                    },
                )
            },
            bottomBar = {
                NavigationBar {
                    Destination.entries.forEachIndexed { index, item ->
                        NavigationBarItem(
                            selected = selectedIndex == index,
                            onClick = { selectedIndex = index },
                            icon = {
                                Icon(
                                    when (item) {
                                        Destination.Overview -> Icons.Default.Home
                                        Destination.Assistant -> Icons.Default.SmartToy
                                        Destination.Logs -> Icons.AutoMirrored.Filled.Article
                                        Destination.Updates -> Icons.Default.SystemUpdate
                                    },
                                    contentDescription = item.title,
                                )
                            },
                            label = { Text(item.title) },
                        )
                    }
                }
            },
        ) { padding ->
            when (destination) {
                Destination.Overview -> OverviewScreen(padding)
                Destination.Assistant -> AiScreen(padding)
                Destination.Logs -> LogsScreen(padding)
                Destination.Updates -> UpdatesScreen(
                    padding = padding,
                    updateState = updateState,
                    onCheck = {
                        updateState = UpdateState.Checking
                        updateScope.launch {
                            updateState = withContext(Dispatchers.IO) { checkForUpdate() }
                        }
                    },
                )
            }
        }
        if (showAbout) AboutDialog(onDismiss = { showAbout = false })
    }
}

@Composable
private fun AboutDialog(onDismiss: () -> Unit) {
    val context = LocalContext.current
    var showLicense by remember { mutableStateOf(false) }
    val versionText = "雀魂 Max Hook ${BuildConfig.VERSION_NAME} (${BuildConfig.VERSION_CODE}) · " +
        "${BuildConfig.UPDATE_CHANNEL} · ${BuildConfig.BUILD_SHA} · ${BuildConfig.APPLICATION_ID}"
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("关于雀魂 Max Hook") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                InfoRow("应用", "雀魂 Max Hook")
                InfoRow("版本", "${BuildConfig.VERSION_NAME} · ${BuildConfig.VERSION_CODE}")
                InfoRow("发布通道", BuildConfig.UPDATE_CHANNEL)
                InfoRow("构建提交", BuildConfig.BUILD_SHA)
                InfoRow("包名", BuildConfig.APPLICATION_ID)
                InfoRow("开发者", "YeFeng233")
                TextButton(onClick = { openUrl(context, "https://github.com/YeFeng233/lsp-majsoul-unlock") }) {
                    Text("项目地址 · GitHub")
                }
                TextButton(onClick = {
                    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                    clipboard.setPrimaryClip(ClipData.newPlainText("版本信息", versionText))
                }) { Text("复制版本信息") }
                TextButton(onClick = { showLicense = true }) { Text("开源许可") }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text("关闭") } },
    )
    if (showLicense) {
        val license = remember {
            listOf("licenses/Akagi-NOTICE.txt", "licenses/Akagi-LICENSE.txt").joinToString("\n\n") { name ->
                runCatching { context.assets.open(name).bufferedReader().use { it.readText() } }.getOrDefault(name)
            }
        }
        AlertDialog(
            onDismissRequest = { showLicense = false },
            title = { Text("Akagi 开源许可") },
            text = { Text(license, Modifier.verticalScroll(rememberScrollState()), style = MaterialTheme.typography.bodySmall) },
            confirmButton = { TextButton(onClick = { showLicense = false }) { Text("关闭") } },
        )
    }
}

@Composable
private fun OverviewScreen(padding: PaddingValues) {
    val context = LocalContext.current
    var status by remember { mutableStateOf(readStatus(context)) }
    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(padding)
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 20.dp, vertical = 12.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        StatusCard(status) { status = readStatus(context) }
        Card(colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface)) {
            Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text("当前安装", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                InfoRow("模块版本", "${BuildConfig.VERSION_NAME} (${BuildConfig.VERSION_CODE})")
                InfoRow("目标游戏", "com.soulgamechst.majsoul")
                InfoRow("更新通道", if (BuildConfig.UPDATE_OWNER.isBlank()) "尚未配置新仓库" else BuildConfig.UPDATE_CHANNEL)
            }
        }
        Card(
            onClick = { ManagerDiagnostics.append(context, "INFO", "ui", "MOD 设置仍由游戏内页面管理") },
            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.primaryContainer),
        ) {
            Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Default.Info, contentDescription = null, tint = MaterialTheme.colorScheme.primary)
                Spacer(Modifier.width(12.dp))
                Column(Modifier.weight(1f)) {
                    Text("MOD 选项请在游戏内设置中调整", fontWeight = FontWeight.Medium)
                    Text("打开雀魂后进入 设置 → MOD设置", style = MaterialTheme.typography.bodySmall)
                }
            }
        }
    }
}

@Composable
private fun StatusCard(status: ModuleStatus, onRefresh: () -> Unit) {
    val context = LocalContext.current
    val installed = runCatching {
        context.packageManager.getLaunchIntentForPackage("com.soulgamechst.majsoul") != null
    }.getOrDefault(false)
    Card(colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface)) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(
                    if (status.diagnosticAvailable) Icons.Default.CheckCircle else Icons.Default.Warning,
                    contentDescription = null,
                    tint = if (status.diagnosticAvailable) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.tertiary,
                    modifier = Modifier.size(28.dp),
                )
                Spacer(Modifier.width(10.dp))
                Column {
                    Text(
                        if (status.diagnosticAvailable) "已有最近诊断记录" else "尚未读取游戏运行记录",
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = FontWeight.SemiBold,
                    )
                    Text(
                        status.lastEvent ?: "启动游戏并在 LSPosed 中启用模块后可产生记录",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            HorizontalDivider()
            InfoRow("目标游戏", if (installed) "已安装" else "未检测到")
            InfoRow("日志来源", "Hook 事件通过游戏 UID 校验通道提交；不读取全局 logcat")
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Button(onClick = {
                    ManagerDiagnostics.append(context, "INFO", "ui", "启动目标游戏")
                    context.packageManager.getLaunchIntentForPackage("com.soulgamechst.majsoul")?.let { context.startActivity(it) }
                }, enabled = installed) { Text("打开雀魂") }
                TextButton(onClick = {
                    ManagerDiagnostics.append(context, "INFO", "ui", "刷新诊断状态")
                    onRefresh()
                }) { Icon(Icons.Default.Refresh, contentDescription = null); Spacer(Modifier.width(4.dp)); Text("刷新诊断") }
            }
        }
    }
}

@Composable
private fun LogsScreen(padding: PaddingValues) {
    val context = LocalContext.current
    var selectedTab by rememberSaveable { mutableIntStateOf(0) }
    var hookLines by remember { mutableStateOf(DiagnosticsStore.read(context, DiagnosticKind.Hook)) }
    var assistantLines by remember { mutableStateOf(ManagerDiagnostics.read(context)) }
    var hookQuery by rememberSaveable { mutableStateOf("") }
    var assistantQuery by rememberSaveable { mutableStateOf("") }
    var hookLevel by rememberSaveable { mutableStateOf("全部") }
    var assistantLevel by rememberSaveable { mutableStateOf("全部") }
    var followLatest by rememberSaveable { mutableStateOf(true) }
    val hookScroll = rememberLazyListState()
    val assistantScroll = rememberLazyListState()
    var showExport by remember { mutableStateOf(false) }
    val lines = if (selectedTab == 0) hookLines else assistantLines
    val query = if (selectedTab == 0) hookQuery else assistantQuery
    val level = if (selectedTab == 0) hookLevel else assistantLevel
    val visible = lines.filter { log ->
        (level == "全部" || log.level == level) &&
            (query.isBlank() || "${log.component} ${log.code} ${log.fields}".contains(query, ignoreCase = true))
    }
    val activeScroll = if (selectedTab == 0) hookScroll else assistantScroll
    LaunchedEffect(selectedTab, visible.size, followLatest) {
        if (followLatest && visible.isNotEmpty()) activeScroll.animateScrollToItem(visible.lastIndex)
    }
    fun refresh() {
        hookLines = DiagnosticsStore.read(context, DiagnosticKind.Hook)
        assistantLines = ManagerDiagnostics.read(context)
    }
    Column(Modifier.fillMaxSize().padding(padding)) {
        Column(Modifier.padding(horizontal = 20.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text("日志", style = MaterialTheme.typography.headlineSmall, modifier = Modifier.weight(1f))
                IconButton(onClick = ::refresh) { Icon(Icons.Default.Refresh, "刷新") }
                IconButton(onClick = { showExport = true }) { Icon(Icons.Default.Share, "打包发送") }
            }
            Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                listOf("Hook 日志", "助手日志").forEachIndexed { index, label ->
                    FilterChip(selected = selectedTab == index, onClick = { selectedTab = index }, label = { Text(label) })
                }
                FilterChip(selected = followLatest, onClick = { followLatest = !followLatest }, label = { Text("跟随最新") })
            }
            OutlinedTextField(
                value = query,
                onValueChange = { if (selectedTab == 0) hookQuery = it else assistantQuery = it },
                modifier = Modifier.fillMaxWidth(),
                singleLine = true,
                label = { Text("搜索消息或事件") },
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Text),
            )
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                listOf("全部", "INFO", "WARN", "ERROR").forEach { level ->
                    FilterChip(selected = level == (if (selectedTab == 0) hookLevel else assistantLevel),
                        onClick = { if (selectedTab == 0) hookLevel = level else assistantLevel = level },
                        label = { Text(level) })
                }
            }
            Text(
                if (lines.isEmpty()) {
                    if (selectedTab == 0) "尚未收到游戏 Hook 事件；请确认 LSPosed 已启用模块并重启游戏" else "助手尚未运行，或还没有产生诊断事件"
                } else "已载入 ${visible.size} / ${lines.size} 条",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        if (visible.isEmpty()) {
            EmptyState("暂无匹配日志", if (lines.isEmpty()) "可刷新页面，或在游戏/助手运行后再次查看。" else "调整搜索内容或级别筛选。")
        } else {
            LazyColumn(
                modifier = Modifier.weight(1f).fillMaxWidth(),
                state = activeScroll,
                contentPadding = PaddingValues(horizontal = 20.dp, vertical = 8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                items(visible) { log ->
                    var expanded by rememberSaveable(log.session, log.seq, log.code) { mutableStateOf(false) }
                    Card(
                        modifier = Modifier.clickable { expanded = !expanded },
                        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface),
                    ) {
                        Column(Modifier.padding(12.dp)) {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Text(log.displayTime, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                                Spacer(Modifier.width(8.dp))
                                Text(log.level, style = MaterialTheme.typography.labelSmall, fontWeight = FontWeight.Bold)
                                Spacer(Modifier.width(8.dp))
                                Text(log.component, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.primary)
                            }
                            Spacer(Modifier.height(4.dp))
                            Text(log.summary)
                            if (expanded) {
                                Text("事件：${log.code} · 会话：${log.session} · 序号：${log.seq}",
                                    style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                                if (log.fields.length() > 0) {
                                    Text("诊断字段：${log.fields}", style = MaterialTheme.typography.bodySmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant)
                                }
                            } else {
                                Text("点按查看诊断详情", style = MaterialTheme.typography.labelSmall,
                                    color = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                        }
                    }
                }
            }
        }
    }
    if (showExport) {
        var range by remember { mutableIntStateOf(0) }
        val options = listOf("最近 1 小时", "最近 24 小时", "全部保留日志")
        val durations = listOf(60 * 60 * 1000L, 24 * 60 * 60 * 1000L, null)
        val previewRange = durations[range]
        val now = System.currentTimeMillis()
        val hookExport = hookLines.filter { previewRange == null || now - it.timeUtcMs <= previewRange }
        val assistantExport = assistantLines.filter { previewRange == null || now - it.timeUtcMs <= previewRange }
        val estimateBytes = DiagnosticsStore.estimateBytes(hookExport, assistantExport)
        val estimateLabel = if (estimateBytes < 1024 * 1024) "${estimateBytes / 1024} KiB"
            else "${"%.1f".format(estimateBytes / (1024f * 1024f))} MiB"
        var exportBusy by remember { mutableStateOf(false) }
        val scope = rememberCoroutineScope()
        AlertDialog(
            onDismissRequest = { showExport = false },
            title = { Text("打包发送") },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    options.forEachIndexed { index, label ->
                        FilterChip(selected = range == index, onClick = { range = index }, label = { Text(label) })
                    }
                    Text("故障包包含 Hook 日志 ${hookExport.size} 条、助手日志 ${assistantExport.size} 条，与当前搜索筛选无关。")
                    Text("预计未压缩约 $estimateLabel；分享前会再次执行字段白名单检查，文件上限 8 MiB。",
                        style = MaterialTheme.typography.bodySmall)
                }
            },
            confirmButton = {
                TextButton(onClick = {
                    if (!exportBusy) scope.launch {
                        exportBusy = true
                        runCatching { withContext(Dispatchers.IO) { DiagnosticsStore.packageZip(context, durations[range]) } }
                            .onSuccess { file ->
                                context.startActivity(Intent.createChooser(DiagnosticsStore.shareIntent(context, file), "发送故障排查包"))
                                showExport = false
                            }
                            .onFailure { ManagerDiagnostics.append(context, "ERROR", "diagnostics", "故障包生成失败") }
                        exportBusy = false
                    }
                }, enabled = !exportBusy) { Text(if (exportBusy) "正在打包…" else "创建并分享") }
            },
            dismissButton = { TextButton(onClick = { showExport = false }) { Text("取消") } },
        )
    }
}

@Composable
private fun UpdatesScreen(
    padding: PaddingValues,
    updateState: UpdateState,
    onCheck: () -> Unit,
) {
    val context = LocalContext.current
    val configured = BuildConfig.UPDATE_OWNER.isNotBlank() && BuildConfig.UPDATE_REPO.isNotBlank()
    Column(
        Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()).padding(20.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Text("更新", style = MaterialTheme.typography.headlineSmall)
        Card(colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface)) {
            Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Image(painterResource(R.drawable.app_icon), contentDescription = null, Modifier.size(52.dp))
                    Spacer(Modifier.width(12.dp))
                    Column {
                        Text("雀魂 Max Hook", fontWeight = FontWeight.SemiBold)
                        Text("${BuildConfig.VERSION_NAME} · ${BuildConfig.UPDATE_CHANNEL}", style = MaterialTheme.typography.bodySmall)
                    }
                }
                HorizontalDivider()
                if (configured) {
                    Text(updateState.label(), color = MaterialTheme.colorScheme.primary)
                    Button(
                        enabled = updateState !is UpdateState.Checking,
                        onClick = onCheck,
                    ) {
                        Icon(Icons.Default.SystemUpdate, contentDescription = null)
                        Spacer(Modifier.width(8.dp))
                        Text(if (updateState is UpdateState.Checking) "检查中…" else "检查更新")
                    }
                    when (val state = updateState) {
                        is UpdateState.Available -> {
                            Text("发现新版本 ${state.versionName}", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                            if (state.notes.isNotBlank()) Text(state.notes.take(500), maxLines = 6)
                            TextButton(onClick = { openUrl(context, state.pageUrl) }) { Text("打开发布页面") }
                        }
                        is UpdateState.Error -> Text(state.message, color = MaterialTheme.colorScheme.error)
                        else -> Unit
                    }
                } else {
                    Text("尚未配置新仓库", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
                    Text("当前构建没有配置 GitHub Releases 更新源。", color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }
        Text("安装新模块后需重启游戏，才能载入新的 Hook 版本。", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

private fun UpdateState.label(): String = when (this) {
    UpdateState.NotConfigured -> "尚未配置新仓库"
    UpdateState.Idle -> "更新源已配置，可以检查新版本。"
    UpdateState.Checking -> "正在检查新仓库…"
    UpdateState.UpToDate -> "当前已是最新稳定版。"
    is UpdateState.Available -> "发现可用更新。"
    is UpdateState.Error -> "检查失败。"
}

private fun checkForUpdate(): UpdateState {
    val owner = BuildConfig.UPDATE_OWNER.trim()
    val repo = BuildConfig.UPDATE_REPO.trim()
    if (owner.isBlank() || repo.isBlank()) return UpdateState.NotConfigured
    val validPart = Regex("[A-Za-z0-9_.-]+")
    if (!validPart.matches(owner) || !validPart.matches(repo)) return UpdateState.Error("更新源配置无效。")
    val endpoint = "https://api.github.com/repos/$owner/$repo/releases?per_page=100"
    return runCatching {
        val releases = requestText(endpoint, "application/vnd.github+json")
        val array = JSONArray(releases)
        var sawRelease = false
        var sawMetadata = false
        var best: UpdateState.Available? = null
        for (index in 0 until array.length()) {
            val release = array.optJSONObject(index) ?: continue
            if (release.optBoolean("draft") || release.optBoolean("prerelease")) continue
            val tag = release.optString("tag_name")
            if (!tag.startsWith("v")) continue
            sawRelease = true
            val assets = release.optJSONArray("assets") ?: continue
            var metadataUrl: String? = null
            for (assetIndex in 0 until assets.length()) {
                val asset = assets.optJSONObject(assetIndex) ?: continue
                if (asset.optString("name") == "hook-update.json") {
                    metadataUrl = asset.optString("browser_download_url").takeIf(String::isNotBlank)
                    break
                }
            }
            val metadata = metadataUrl?.takeIf(::isAllowedGitHubUrl)
                ?.let { requestText(it, "application/octet-stream") } ?: continue
            sawMetadata = true
            val info = JSONObject(metadata)
            if (info.optString("applicationId") != BuildConfig.APPLICATION_ID) continue
            if (info.optString("channel", BuildConfig.UPDATE_CHANNEL) != BuildConfig.UPDATE_CHANNEL) continue
            if (info.optInt("versionCode", -1) <= BuildConfig.VERSION_CODE) continue
            val candidate = UpdateState.Available(
                versionCode = info.optInt("versionCode", -1),
                versionName = info.optString("versionName", tag.removePrefix("v")),
                notes = release.optString("body"),
                pageUrl = release.optString("html_url"),
            )
            if (best == null || candidate.versionCode > best!!.versionCode) best = candidate
        }
        best ?: when {
            !sawRelease -> UpdateState.UpToDate
            !sawMetadata -> UpdateState.Error("发布中没有符合协议的 hook-update.json。")
            else -> UpdateState.UpToDate
        }
    }.getOrElse { error ->
        UpdateState.Error(error.message?.take(160) ?: "无法连接更新源。")
    }
}

private fun requestText(url: String, accept: String): String {
    val connection = (URL(url).openConnection() as HttpURLConnection).apply {
        connectTimeout = 10_000
        readTimeout = 10_000
        requestMethod = "GET"
        setRequestProperty("Accept", accept)
        setRequestProperty("User-Agent", "MajsoulMax-Hook/${BuildConfig.VERSION_NAME}")
    }
    return try {
        if (connection.responseCode !in 200..299) error("更新源返回 HTTP ${connection.responseCode}")
        connection.inputStream.bufferedReader().use { it.readText().take(2 * 1024 * 1024) }
    } finally {
        connection.disconnect()
    }
}

private fun openUrl(context: Context, url: String) {
    if (!url.startsWith("https://github.com/")) return
    runCatching {
        context.startActivity(android.content.Intent(android.content.Intent.ACTION_VIEW, android.net.Uri.parse(url)))
    }
}

private fun isAllowedGitHubUrl(value: String): Boolean {
    val uri = android.net.Uri.parse(value)
    return uri.scheme == "https" && (uri.host == "github.com" || uri.host?.endsWith(".githubusercontent.com") == true)
}

@Composable
private fun EmptyState(title: String, message: String) {
    Box(Modifier.fillMaxSize().padding(24.dp), contentAlignment = Alignment.Center) {
        Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Icon(Icons.AutoMirrored.Filled.Article, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.size(40.dp))
            Text(title, style = MaterialTheme.typography.titleMedium)
            Text(message, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

@Composable
private fun InfoRow(label: String, value: String) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
        Text(label, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.weight(1f))
        Text(value, fontWeight = FontWeight.Medium)
    }
}

@Composable
private fun HookManagerTheme(content: @Composable () -> Unit) {
    val colors = if (androidx.compose.foundation.isSystemInDarkTheme()) {
        darkColorScheme(primary = Color(0xFF78D8A4), onPrimary = Color(0xFF003920), background = Color(0xFF101813), surface = Color(0xFF1B261F))
    } else {
        lightColorScheme(primary = Green, background = Cream, surface = Color(0xFFFFFEF8), primaryContainer = Color(0xFFD8F0DF))
    }
    MaterialTheme(colorScheme = colors, content = content)
}

private fun readStatus(context: Context): ModuleStatus {
    val last = ManagerDiagnostics.read(context).lastOrNull()
    return ModuleStatus(last != null, last?.let { "${it.displayTime} · ${it.summary}" })
}
