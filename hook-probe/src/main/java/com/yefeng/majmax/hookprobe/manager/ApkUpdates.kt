package com.yefeng.majmax.hookprobe.manager

import android.content.Context
import android.content.Intent
import android.content.pm.PackageInfo
import android.content.pm.PackageManager
import android.net.Uri
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.ui.Modifier
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import androidx.core.content.FileProvider
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.yefeng.majmax.hookprobe.BuildConfig
import java.io.File
import java.security.MessageDigest
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

internal data class ApkRelease(val versionCode: Int, val url: String, val bytes: Long = 0, val sha256: String = "")
internal data class ApkTransfer(
    val release: ApkRelease? = null, val downloading: Boolean = false,
    val bytes: Long = 0, val total: Long = 0, val file: File? = null, val error: String? = null,
)

internal object ApkUpdates {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val mutableState = MutableStateFlow(ApkTransfer())
    val state = mutableState.asStateFlow()
    private var job: Job? = null

    @Synchronized
    fun download(context: Context, release: ApkRelease) {
        if (job?.isActive == true) return
        val app = context.applicationContext
        mutableState.value = ApkTransfer(release, downloading = true)
        job = scope.launch {
            val folder = File(app.cacheDir, "updates")
            val temporary = File(folder, "update.part")
            val ready = File(folder, "update-${release.versionCode}.apk")
            try {
                require(release.url.startsWith("https://github.com/${BuildConfig.UPDATE_OWNER}/${BuildConfig.UPDATE_REPO}/releases/download/") &&
                    GitHubConnection.isOrigin(release.url)) { "安装包必须来自当前仓库的发布附件" }
                check(folder.isDirectory || folder.mkdirs()) { "无法创建更新目录" }
                if (!ready.isFile || runCatching { verify(app, ready, release) }.isFailure) {
                    var lastProgress = 0L
                    GitHubConnection.downloadApk(app, release.url, temporary, progress = { bytes, total ->
                        coroutineContext.ensureActive()
                        val now = android.os.SystemClock.elapsedRealtime()
                        if (bytes == 0L || now - lastProgress >= 200 || bytes == total) {
                            mutableState.value = ApkTransfer(release, true, bytes, total.takeIf { it > 0 } ?: release.bytes)
                            lastProgress = now
                        }
                    }, validate = { verify(app, it, release) })
                    coroutineContext.ensureActive()
                    check(!ready.exists() || ready.delete()) { "无法替换安装包" }
                    check(temporary.renameTo(ready)) { "无法保存安装包" }
                }
                coroutineContext.ensureActive()
                // Keep only the current verified package; other app data is outside this directory.
                folder.listFiles()?.filter { it != ready && it.name.startsWith("update-") && it.extension == "apk" }
                    ?.forEach { it.delete() }
                mutableState.value = ApkTransfer(release, file = ready, bytes = ready.length(), total = ready.length())
            } catch (cancelled: CancellationException) {
                mutableState.value = ApkTransfer(release)
                throw cancelled
            } catch (error: Exception) {
                mutableState.value = ApkTransfer(release, error = (error.cause?.message ?: error.message)?.take(180) ?: "下载失败，请重试")
            } finally {
                temporary.delete()
            }
        }
    }

    @Synchronized
    fun cancel() { job?.cancel() }

    @Synchronized
    fun discardInvalid(release: ApkRelease) {
        val current = mutableState.value
        if (current.release == release && !current.downloading) {
            current.file?.delete()
            mutableState.value = ApkTransfer(release, error = "缓存安装包已失效，请重新下载。")
        }
    }

    @Suppress("DEPRECATION")
    internal fun verify(context: Context, file: File, release: ApkRelease) {
        check(file.isFile && file.length() in 1..512L * 1024 * 1024) { "安装包为空或大小无效" }
        check(release.bytes <= 0 || file.length() == release.bytes) { "安装包大小与发布记录不一致" }
        if (release.sha256.isNotBlank()) {
            val digest = MessageDigest.getInstance("SHA-256")
            file.inputStream().buffered().use { input ->
                val buffer = ByteArray(64 * 1024)
                while (true) {
                    val count = input.read(buffer)
                    if (count < 0) break
                    digest.update(buffer, 0, count)
                }
            }
            val actual = digest.digest().joinToString("") { "%02x".format(it) }
            check(actual.equals(release.sha256, ignoreCase = true)) { "安装包校验失败，请重新下载" }
        }
        val pm = context.packageManager
        val archive = checkNotNull(pm.getPackageArchiveInfo(file.absolutePath, PackageManager.GET_SIGNING_CERTIFICATES)) {
            "下载内容不是有效的 APK 安装包"
        }
        val installed = pm.getPackageInfo(context.packageName, PackageManager.GET_SIGNING_CERTIFICATES)
        validateIdentity(context.packageName, installed.longVersionCode, archive, release.versionCode)
        val currentSigners = installed.signingInfo?.apkContentsSigners?.map { it.toCharsString() }?.toSet().orEmpty()
        val nextSigners = archive.signingInfo?.apkContentsSigners?.map { it.toCharsString() }?.toSet().orEmpty()
        check(currentSigners.isNotEmpty() && currentSigners == nextSigners) { "安装包签名与当前应用不一致，无法覆盖更新" }
    }

    internal fun validateIdentity(packageName: String, currentVersion: Long, archive: PackageInfo, expectedVersion: Int) {
        check(archive.packageName == packageName) { "安装包不属于当前应用" }
        check(archive.longVersionCode == expectedVersion.toLong() && archive.longVersionCode > currentVersion) {
            "安装包版本与更新信息不一致，或不是更新版本"
        }
        check((archive.applicationInfo?.minSdkVersion ?: Int.MAX_VALUE) <= android.os.Build.VERSION.SDK_INT) {
            "当前 Android 版本不支持此更新"
        }
    }

    fun installIntent(context: Context, file: File): Intent {
        val uri = FileProvider.getUriForFile(context, "${context.packageName}.updates", file)
        return Intent(Intent.ACTION_VIEW).setDataAndType(uri, "application/vnd.android.package-archive")
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
    }
}

@Composable
internal fun ApkUpdateActions(release: ApkRelease) {
    val context = LocalContext.current
    val transfer by ApkUpdates.state.collectAsStateWithLifecycle()
    val current = transfer.takeIf { it.release == release } ?: ApkTransfer(release)
    val scope = rememberCoroutineScope()
    var installing by remember(release) { mutableStateOf(false) }
    var installError by remember(release) { mutableStateOf<String?>(null) }
    fun install() {
        val file = current.file ?: return
        installing = true
        installError = null
        scope.launch {
            val verified = runCatching { withContext(Dispatchers.IO) { ApkUpdates.verify(context, file, release) } }
            // Leaving this screen cancels its UI scope, not the verified cached package.
            (verified.exceptionOrNull() as? CancellationException)?.let { throw it }
            if (verified.isFailure) {
                ApkUpdates.discardInvalid(release)
                installError = verified.exceptionOrNull()?.message ?: "安装包校验失败，请重新下载"
                installing = false
                return@launch
            }
            runCatching {
                context.startActivity(ApkUpdates.installIntent(context, file))
            }.onFailure { installError = it.message ?: "无法打开系统安装界面" }
            installing = false
        }
    }
    val permission = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) {
        if (context.packageManager.canRequestPackageInstalls()) install()
        else installError = "尚未允许安装应用，可再次点击安装更新。"
    }
    when {
        current.downloading -> {
            if (current.total > 0) {
                LinearProgressIndicator(progress = { (current.bytes.toFloat() / current.total).coerceIn(0f, 1f) }, modifier = Modifier.fillMaxWidth())
                Text("正在下载：${current.bytes * 100 / current.total}% · ${"%.1f".format(current.bytes / 1048576.0)} / ${"%.1f".format(current.total / 1048576.0)} MiB")
            } else {
                LinearProgressIndicator(Modifier.fillMaxWidth())
                Text("正在下载：${"%.1f".format(current.bytes / 1048576.0)} MiB")
            }
            ManagerOutlinedButton(onClick = { ApkUpdates.cancel() }) { Text("取消下载") }
        }
        current.file != null -> {
            Text("安装包已下载并校验，可以安装更新。")
            ManagerButton(enabled = !installing, onClick = {
                installError = null
                if (context.packageManager.canRequestPackageInstalls()) install()
                else runCatching {
                    permission.launch(Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${context.packageName}")))
                }.onFailure { installError = "无法打开安装权限设置，请在系统设置中允许此应用安装更新。" }
            }) { Text(if (installing) "正在准备安装…" else "安装更新") }
        }
        else -> ManagerButton(enabled = !transfer.downloading, onClick = { ApkUpdates.download(context, release) }) {
            Text(if (current.error == null) "下载更新" else "重新下载")
        }
    }
    current.error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
    installError?.let { Text(it, color = MaterialTheme.colorScheme.error) }
}
