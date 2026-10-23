package com.yefeng.majmax.hookprobe.manager

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import java.text.DateFormat
import java.util.Date

@Composable
internal fun UnlockCatalogCard() {
    val context = LocalContext.current
    val status by CatalogUpdates.state.collectAsStateWithLifecycle()
    Card(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text("皮肤与装扮解锁清单", style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold)
            Text("本地版本：${status.version} · ${status.skins} 套皮肤")
            Text(status.message, color = if (status.error) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurface)
            if (status.checkedAt > 0) Text("上次检查：${DateFormat.getDateTimeInstance().format(Date(status.checkedAt))}",
                style = MaterialTheme.typography.bodySmall)
            Text(if (status.restartRequired) "完全退出并重新启动雀魂后载入最新清单。" else "最新清单已在游戏启动时载入。",
                style = MaterialTheme.typography.bodySmall)
            Button(onClick = { CatalogUpdates.check(context) }, enabled = !status.checking) {
                Text(if (status.checking) "正在检查清单…" else "立即更新解锁清单")
            }
            Text("每次应用启动自动检查仓库版本；有新版本时自动下载并校验。离线或检查失败会保留本地清单，不影响角色、皮肤选择和 MOD 设置。",
                style = MaterialTheme.typography.bodySmall)
        }
    }
}

@Composable
internal fun GitHubConnectionCard() {
    val context = LocalContext.current
    var saved by remember { mutableStateOf(GitHubConnection.settings(context)) }
    var enabled by remember { mutableStateOf(saved.accelerated) }
    var prefix by remember { mutableStateOf(saved.prefix) }
    var message by remember { mutableStateOf("") }
    var error by remember { mutableStateOf(false) }
    Card(Modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Row {
                Text("GitHub 仓库加速", modifier = Modifier.weight(1f), style = MaterialTheme.typography.titleMedium)
                Switch(checked = enabled, onCheckedChange = { enabled = it })
            }
            Text(if (saved.accelerated) "已保存：使用自定义加速地址" else "默认直接连接 GitHub")
            OutlinedTextField(value = prefix, onValueChange = { prefix = it }, singleLine = true,
                label = { Text("加速站地址（HTTPS）") }, keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                modifier = Modifier.fillMaxWidth())
            Text("填写加速服务的地址前缀，应用会在后面追加完整 GitHub 链接。用于清单、版本检查及安装包下载；网络请求失败时会尝试直连。",
                style = MaterialTheme.typography.bodySmall)
            Button(onClick = {
                runCatching { GitHubConnection.save(context, enabled, prefix) }
                    .onSuccess { saved = GitHubConnection.settings(context); message = "连接设置已保存，下次检查和下载时使用"; error = false }
                    .onFailure { message = it.message ?: "地址无效"; error = true }
            }) { Text("保存连接设置") }
            if (message.isNotBlank()) Text(message, color = if (error) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.primary)
        }
    }
}
