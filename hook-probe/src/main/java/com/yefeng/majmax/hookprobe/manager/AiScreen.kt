package com.yefeng.majmax.hookprobe.manager

import android.Manifest
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle

@Composable
internal fun AiScreen(padding: PaddingValues) {
    val context = LocalContext.current
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    var allowed by remember { mutableStateOf(Settings.canDrawOverlays(context)) }
    var showLicense by remember { mutableStateOf(false) }
    var fourModel by remember { mutableStateOf(OnnxModelStore.info(context, 4)) }
    var threeModel by remember { mutableStateOf(OnnxModelStore.info(context, 3)) }
    var modelMessage by remember { mutableStateOf("仅接受符合 Akagi 策略协议的 ONNX 文件；PyTorch/Mortal 权重包不能只改后缀导入。模型仅保存在本机；和牌率、向听与放铳风险仍由 Akagi 分析计算。") }
    val scope = rememberCoroutineScope()
    val status by AiStatus.state.collectAsStateWithLifecycle()
    val notificationPermission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {}
    val modelPicker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) scope.launch {
            modelMessage = "正在复制并检查模型…"
            runCatching { withContext(Dispatchers.IO) { OnnxModelStore.import(context, uri) } }
                .onSuccess { imported ->
                    fourModel = OnnxModelStore.info(context, 4)
                    threeModel = OnnxModelStore.info(context, 3)
                    runCatching { AiNative.configurePolicy(OnnxModelStore.enabledMask(context)) }
                    modelMessage = "${imported.players} 人模型已校验并暂存；下一次完整牌局同步时生效。"
                }
                .onFailure { error -> modelMessage = error.message?.take(160) ?: "模型导入失败，继续使用内置模型。" }
        }
    }
    DisposableEffect(lifecycle) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) allowed = Settings.canDrawOverlays(context)
        }
        lifecycle.addObserver(observer)
        onDispose { lifecycle.removeObserver(observer) }
    }
    fun start(demo: Boolean) {
        if (!Settings.canDrawOverlays(context)) {
            context.startActivity(Intent(Settings.ACTION_MANAGE_OVERLAY_PERMISSION, Uri.parse("package:${context.packageName}")))
            return
        }
        AiOverlayService.start(context, demo)
        if (Build.VERSION.SDK_INT >= 33) notificationPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
    }
    Column(Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()).padding(20.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp)) {
        Text("本地牌局助手", style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.SemiBold)
        Text("实时推荐切牌，展示听牌后的和牌率估计与放铳风险。AI 和牌局分析均在手机内完成。",
            style = MaterialTheme.typography.bodyMedium)
        Card {
            Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text(if (status.running) "助手已开启" else "助手未开启", style = MaterialTheme.typography.titleMedium)
                Text(status.message)
                Text(if (allowed) "悬浮窗权限已允许" else "首次使用需允许“显示在其他应用上层”", style = MaterialTheme.typography.bodySmall)
                Button(onClick = { start(false) }) { Text(if (allowed) "开启实时悬浮助手" else "允许悬浮窗权限") }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    OutlinedButton(onClick = { start(true) }) { Text("本地模型自检") }
                    TextButton(onClick = { AiOverlayService.stop(context) }, enabled = status.running) { Text("停止助手") }
                }
            }
        }
        Text("策略模型", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
        PolicyModelCard(
            players = 4, model = fourModel, selectedCustom = OnnxModelStore.selectedCustom(context, 4),
            message = modelMessage,
            onImport = { modelPicker.launch(arrayOf("*/*")) },
            onBuiltIn = {
                OnnxModelStore.selectBuiltIn(context, 4)
                runCatching { AiNative.configurePolicy(OnnxModelStore.enabledMask(context)) }
                modelMessage = "四麻已选择内置策略；当前牌局保持原模型，下一次完整牌局同步时生效。"
            },
            onCustom = {
                runCatching {
                    OnnxModelStore.selectCustom(context, 4)
                    AiNative.configurePolicy(OnnxModelStore.enabledMask(context))
                    modelMessage = "四麻自定义策略将在下一次完整牌局同步时生效。"
                }.onFailure { modelMessage = it.message ?: "无法启用模型" }
            },
            onDelete = {
                OnnxModelStore.delete(context, 4); fourModel = null
                runCatching { AiNative.configurePolicy(OnnxModelStore.enabledMask(context)) }
                modelMessage = "四麻自定义模型已从可选槽位移除。"
            },
            onSelfTest = {
                scope.launch {
                    modelMessage = withContext(Dispatchers.IO) {
                        runCatching { OnnxModelStore.selfTest(context, 4) }.getOrElse { it.message ?: "自检失败" }
                    }
                }
            },
        )
        PolicyModelCard(
            players = 3, model = threeModel, selectedCustom = OnnxModelStore.selectedCustom(context, 3),
            message = modelMessage,
            onImport = { modelPicker.launch(arrayOf("*/*")) },
            onBuiltIn = {
                OnnxModelStore.selectBuiltIn(context, 3)
                runCatching { AiNative.configurePolicy(OnnxModelStore.enabledMask(context)) }
                modelMessage = "三麻已选择内置策略；当前牌局保持原模型，下一次完整牌局同步时生效。"
            },
            onCustom = {
                runCatching {
                    OnnxModelStore.selectCustom(context, 3)
                    AiNative.configurePolicy(OnnxModelStore.enabledMask(context))
                    modelMessage = "三麻自定义策略将在下一次完整牌局同步时生效。"
                }.onFailure { modelMessage = it.message ?: "无法启用模型" }
            },
            onDelete = {
                OnnxModelStore.delete(context, 3); threeModel = null
                runCatching { AiNative.configurePolicy(OnnxModelStore.enabledMask(context)) }
                modelMessage = "三麻自定义模型已从可选槽位移除。"
            },
            onSelfTest = {
                scope.launch {
                    modelMessage = withContext(Dispatchers.IO) {
                        runCatching { OnnxModelStore.selfTest(context, 3) }.getOrElse { it.message ?: "自检失败" }
                    }
                }
            },
        )
        Card {
            Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Text("悬浮窗操作", fontWeight = FontWeight.SemiBold)
                Text("拖动标题栏移动窗口。点“调节”设置透明度和宽度，点“收纳”变成 AI 小浮标，点浮标再次展开。位置和显示设置会自动保存。")
                Text("先开启助手，再进入雀魂对局。更新模块后请重启游戏；中途开启或丢失消息时，重新进入牌局以恢复完整状态。")
                TextButton(onClick = {
                    context.packageManager.getLaunchIntentForPackage(AiOverlayService.GAME)?.let(context::startActivity)
                }) { Text("打开雀魂") }
            }
        }
        Card {
            Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Text("如何理解这些指标", fontWeight = FontWeight.SemiBold)
                Text("和牌率仅在听牌时显示；未听牌时显示向听数和进张。放铳风险指数包含打点权重，越低越好，并非实际放铳概率。分析只使用己方手牌与公开牌面。")
                Text("使用 Akagi 的本地轻量模型，支持普通四麻和三麻。自检使用内置样例，窗口会明确标记“非当前牌局”。")
                TextButton(onClick = { showLicense = true }) { Text("Akagi 开源许可") }
            }
        }
    }
    if (showLicense) {
        val license = remember {
            listOf("licenses/Akagi-NOTICE.txt", "licenses/Akagi-LICENSE.txt").joinToString("\n\n") { name ->
                runCatching { context.assets.open(name).bufferedReader().use { it.readText() } }.getOrDefault(name)
            }
        }
        AlertDialog(onDismissRequest = { showLicense = false }, title = { Text("Akagi · Apache-2.0") },
            text = { Text(license, Modifier.verticalScroll(rememberScrollState()), style = MaterialTheme.typography.bodySmall) },
            confirmButton = { TextButton(onClick = { showLicense = false }) { Text("关闭") } })
    }
}

@Composable
private fun PolicyModelCard(
    players: Int,
    model: ImportedPolicy?,
    selectedCustom: Boolean,
    message: String,
    onImport: () -> Unit,
    onBuiltIn: () -> Unit,
    onCustom: () -> Unit,
    onDelete: () -> Unit,
    onSelfTest: () -> Unit,
) {
    Card {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(if (players == 4) "四麻模型" else "三麻模型", fontWeight = FontWeight.SemiBold)
            Text("当前：${if (selectedCustom && model != null) "自定义 ONNX" else "内置 Akagi"}")
            if (model != null) {
                Text("${model.name} · ${"%.1f".format(model.bytes / (1024f * 1024f))} MiB · SHA-256 ${model.sha256.take(12)}",
                    style = MaterialTheme.typography.bodySmall)
                Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    TextButton(onClick = onCustom) { Text(if (selectedCustom) "已选自定义" else "使用自定义") }
                    TextButton(onClick = onBuiltIn) { Text("切换内置") }
                }
                Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    OutlinedButton(onClick = onSelfTest) { Text("模型自检") }
                    TextButton(onClick = onDelete) { Text("删除") }
                }
            }
            Button(onClick = onImport) { Text("导入 .onnx") }
            Text(message, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}
