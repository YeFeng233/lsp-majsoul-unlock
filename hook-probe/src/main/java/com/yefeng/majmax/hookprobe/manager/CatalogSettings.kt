package com.yefeng.majmax.hookprobe.manager

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import java.text.DateFormat
import java.util.Date
import android.os.SystemClock
import kotlinx.coroutines.delay

@Composable
internal fun UnlockCatalogCard() {
    val context = LocalContext.current
    val status by CatalogUpdates.state.collectAsStateWithLifecycle()
    ManagerCard {
        Text("皮肤与装扮解锁清单", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
        Text("本地版本：${status.version} · ${status.skins} 套皮肤")
        Text(status.message, color = if (status.error) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurface)
        if (status.checkedAt > 0) Text("上次检查：${DateFormat.getDateTimeInstance().format(Date(status.checkedAt))}",
            style = MaterialTheme.typography.bodySmall)
        Text(if (status.restartRequired) "完全退出并重新启动雀魂后载入最新清单。" else "最新清单已在游戏启动时载入。",
            style = MaterialTheme.typography.bodySmall)
        ManagerButton(onClick = { CatalogUpdates.check(context) }, enabled = !status.checking) {
            Text(if (status.checking) "正在检查清单…" else "立即更新解锁清单")
        }
        Text("每次应用启动自动检查仓库版本；有新版本时自动下载并校验。离线或检查失败会保留本地清单，不影响角色、皮肤选择和 MOD 设置。",
            style = MaterialTheme.typography.bodySmall)
    }
}

@Composable
internal fun GitHubConnectionCard() {
    val context = LocalContext.current
    val speed by GitHubSpeedTests.state.collectAsStateWithLifecycle()
    var saved by remember { mutableStateOf(GitHubConnection.settings(context)) }
    var enabled by remember { mutableStateOf(saved.accelerated) }
    var source by remember { mutableStateOf(
        GitHubConnection.presets.firstOrNull { it.prefix == saved.prefix }?.prefix
            ?: if (saved.prefix.isBlank()) GitHubConnection.presets.first().prefix else ""
    ) }
    var prefix by remember { mutableStateOf(GitHubConnection.customPrefix(context)) }
    var expanded by remember { mutableStateOf(false) }
    var message by remember { mutableStateOf("") }
    var error by remember { mutableStateOf(false) }
    var clock by remember { mutableStateOf(SystemClock.elapsedRealtime()) }
    LaunchedEffect(speed.cooldownUntil) {
        clock = SystemClock.elapsedRealtime()
        while (clock < speed.cooldownUntil) { delay(1_000); clock = SystemClock.elapsedRealtime() }
    }
    LaunchedEffect(speed.completedAt) {
        if (speed.completedAt > 0 && speed.winner != null) {
            saved = GitHubConnection.settings(context)
            enabled = saved.accelerated
            source = GitHubConnection.presets.firstOrNull { it.prefix == saved.prefix }?.prefix ?: ""
            prefix = GitHubConnection.customPrefix(context)
        }
    }
    val cooldownSeconds = ((speed.cooldownUntil - clock + 999) / 1_000).coerceAtLeast(0)
    ManagerCard {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text("GitHub 仓库加速", modifier = Modifier.weight(1f), style = MaterialTheme.typography.titleMedium)
            Switch(checked = enabled, onCheckedChange = { enabled = it }, enabled = !speed.testing)
        }
        Text(if (saved.accelerated) "已保存：${GitHubConnection.presets.firstOrNull { it.prefix == saved.prefix }?.name ?: "自定义加速地址"}" else "默认直接连接 GitHub")
        Text("加速源", style = MaterialTheme.typography.labelLarge)
        Box {
            ManagerOutlinedButton(onClick = { expanded = true }, enabled = !speed.testing, modifier = Modifier.fillMaxWidth()) {
                Text(GitHubConnection.presets.firstOrNull { it.prefix == source }?.name ?: "自定义地址（备选）")
            }
            DropdownMenu(expanded = expanded, onDismissRequest = { expanded = false }) {
                GitHubConnection.presets.forEach { preset ->
                    DropdownMenuItem(text = { Text(preset.name) }, onClick = { source = preset.prefix; expanded = false })
                }
                DropdownMenuItem(text = { Text("自定义地址（备选）") }, onClick = { source = ""; expanded = false })
            }
        }
        if (source.isBlank()) {
            OutlinedTextField(value = prefix, onValueChange = { prefix = it }, singleLine = true,
                enabled = !speed.testing,
                label = { Text("加速站地址（HTTPS）") }, keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                modifier = Modifier.fillMaxWidth())
            Text("填写 HTTPS 地址前缀，应用会在后面追加完整 GitHub 链接。", style = MaterialTheme.typography.bodySmall)
        }
        Text("保存后同时用于解锁清单、更新检查和 APK 下载。优先使用所选加速源，失败后自动尝试直连 GitHub。公共加速服务由第三方提供，可按网络情况切换。",
            style = MaterialTheme.typography.bodySmall)
        ManagerButton(enabled = !speed.testing, onClick = {
            runCatching { GitHubConnection.save(context, enabled, source.ifBlank { prefix }, prefix) }
                .onSuccess { saved = GitHubConnection.settings(context); message = "连接设置已保存，下次检查和下载时使用"; error = false }
                .onFailure { message = it.message ?: "地址无效"; error = true }
        }) { Text("保存连接设置") }
        if (message.isNotBlank()) Text(message, color = if (error) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.primary)
        ManagerButton(enabled = !speed.testing && cooldownSeconds == 0L, onClick = {
            message = ""; error = false
            GitHubSpeedTests.start(context, prefix)
        }, modifier = Modifier.fillMaxWidth()) {
            Text(if (speed.testing) "正在测速…" else if (cooldownSeconds > 0) "${cooldownSeconds} 秒后可重新测速" else "测速并自动选择")
        }
        Text("测试所有预设和有效自定义地址的清单请求耗时，完成后自动启用并保存最快线路；全部失败保留原设置。两次测试至少间隔 30 秒。",
            style = MaterialTheme.typography.bodySmall)
        if (speed.message.isNotBlank()) Text(speed.message,
            color = if (speed.failed) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurface)
        if (speed.completedAt > 0) Text("上次测速：${DateFormat.getDateTimeInstance().format(Date(speed.completedAt))}",
            style = MaterialTheme.typography.bodySmall)
        speed.results.forEach { result ->
            val value = result.latencyMs?.let { "$it ms" } ?: result.error ?: "测试中…"
            Text("${result.source.name}：$value", style = MaterialTheme.typography.bodySmall)
        }
    }
}
