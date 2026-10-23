package com.yefeng.majmax.hookprobe.manager

import android.content.Context
import android.util.AtomicFile
import com.yefeng.majmax.hookprobe.BuildConfig
import com.yefeng.majmax.hookprobe.catalog.UnlockCatalogData
import com.yefeng.majmax.hookprobe.catalog.UnlockCatalogData.Snapshot
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import org.json.JSONObject
import java.io.File

/** One atomic envelope keeps version, checksum and data from different downloads from mixing. */
internal class CatalogRepository(private val root: File, private val bundled: Snapshot, private val moduleVersion: Int) {
    private val cache = AtomicFile(File(root, "unlock-catalog.json"))
    private var loaded: Snapshot? = null

    @Synchronized fun current(): Snapshot {
        loaded?.let { return it }
        val saved = runCatching {
            val envelope = cache.openRead().use {
                JSONObject(UnlockCatalogData.utf8(UnlockCatalogData.readBounded(it, 2 * UnlockCatalogData.MAX_DATA_BYTES)))
            }
            UnlockCatalogData.parse(envelope.getString("manifest"), envelope.getString("payload"), moduleVersion)
        }.getOrNull()
        return (saved?.takeIf { it.version > bundled.version || (it.version == bundled.version && it.sha256 == bundled.sha256) }
            ?: bundled).also { loaded = it }
    }

    fun update(fetch: (String, Int) -> String): Boolean {
        val manifest = fetch("manifest.json", UnlockCatalogData.MAX_MANIFEST_BYTES)
        val info = UnlockCatalogData.manifest(manifest, moduleVersion)
        val before = current()
        if (info.getLong("version") < before.version) return false
        if (info.getLong("version") == before.version) {
            require(info.getString("sha256") == before.sha256) { "仓库清单内容已修改但版本未提高，请等待维护者修正" }
            return false
        }
        val payload = fetch(info.getString("dataFile") + "?version=" + info.getLong("version"), UnlockCatalogData.MAX_DATA_BYTES)
        val candidate = UnlockCatalogData.parse(manifest, payload, moduleVersion)
        synchronized(this) {
            if (candidate.version <= current().version) return false
            check(root.isDirectory || root.mkdirs()) { "无法创建清单缓存目录" }
            val output = cache.startWrite()
            try {
                output.write(candidate.envelope().toByteArray(Charsets.UTF_8))
                cache.finishWrite(output)
            } catch (error: Throwable) {
                cache.failWrite(output); throw error
            }
            loaded = candidate
        }
        return true
    }
}

internal object UnlockCatalogStore {
    @Volatile private var repository: CatalogRepository? = null
    fun repo(context: Context): CatalogRepository = repository ?: synchronized(this) {
        repository ?: CatalogRepository(File(context.filesDir, "unlock-catalog"), bundled(context), BuildConfig.VERSION_CODE)
            .also { repository = it }
    }
    fun bundled(context: Context): Snapshot {
        fun text(name: String, max: Int) = context.assets.open("unlock-catalog/$name").use {
            UnlockCatalogData.utf8(UnlockCatalogData.readBounded(it, max))
        }
        return UnlockCatalogData.parse(text("manifest.json", UnlockCatalogData.MAX_MANIFEST_BYTES),
            text("unlock-data.json", UnlockCatalogData.MAX_DATA_BYTES), BuildConfig.VERSION_CODE)
    }
    fun baseUrl(): String {
        val owner = BuildConfig.UPDATE_OWNER
        val repo = BuildConfig.UPDATE_REPO
        require(Regex("[A-Za-z0-9_.-]+").matches(owner) && Regex("[A-Za-z0-9_.-]+").matches(repo)) { "仓库配置无效" }
        return "https://raw.githubusercontent.com/$owner/$repo/main/catalog/"
    }
}

internal data class CatalogUpdateStatus(
    val checking: Boolean = false, val version: String = "读取中…", val skins: Int = 0,
    val checkedAt: Long = 0, val restartRequired: Boolean = true,
    val message: String = "启动时会自动检查解锁清单", val error: Boolean = false,
)

internal object CatalogUpdates {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val mutable = MutableStateFlow(CatalogUpdateStatus())
    val state = mutable.asStateFlow()
    private var running = false

    @Synchronized fun check(context: Context) {
        if (running) return
        running = true
        val app = context.applicationContext
        mutable.value = mutable.value.copy(checking = true, message = "正在检查仓库解锁清单…", error = false)
        scope.launch {
            try {
                app.getSharedPreferences("unlock-catalog", Context.MODE_PRIVATE).edit()
                    .putLong("checkedAt", System.currentTimeMillis()).apply()
                val repo = UnlockCatalogStore.repo(app)
                publish(app, repo.current(), true, "正在检查仓库解锁清单…")
                val changed = repo.update { name, limit ->
                    // Raw GitHub and accelerators can cache an old manifest for several minutes.
                    val path = if (name == "manifest.json") "$name?check=${System.currentTimeMillis()}" else name
                    GitHubConnection.requestJson(app, UnlockCatalogStore.baseUrl() + path, limit = limit)
                }
                app.getSharedPreferences("unlock-catalog", Context.MODE_PRIVATE).edit()
                    .putLong("checkedAt", System.currentTimeMillis()).apply()
                publish(app, repo.current(), false, if (changed) "解锁清单已更新，完全重启游戏后生效" else "已检查，当前清单无需更新")
                DiagnosticsStore.appendAssistant(app, "INFO", "catalog.update", if (changed) "CATALOG_UPDATED" else "CATALOG_CURRENT")
            } catch (error: Exception) {
                val current = runCatching { UnlockCatalogStore.repo(app).current() }.getOrNull()
                if (current != null) publish(app, current, false, "检查失败，继续使用本地清单：${error.message?.take(100) ?: "网络不可用"}", true)
                else mutable.value = mutable.value.copy(checking = false, error = true, message = "无法读取解锁清单")
                DiagnosticsStore.appendAssistant(app, "WARN", "catalog.update", "CATALOG_UPDATE_FAILED")
            } finally { synchronized(this@CatalogUpdates) { running = false } }
        }
    }

    private fun publish(context: Context, snapshot: Snapshot, checking: Boolean, message: String, error: Boolean = false) {
        val prefs = context.getSharedPreferences("unlock-catalog", Context.MODE_PRIVATE)
        mutable.value = CatalogUpdateStatus(checking, snapshot.versionName, snapshot.skinCount, prefs.getLong("checkedAt", 0),
            prefs.getLong("appliedVersion", 0) != snapshot.version || prefs.getString("appliedHash", "") != snapshot.yamlSha256,
            message, error)
    }

    fun applied(context: Context, version: Long, hash: String) {
        val current = UnlockCatalogStore.repo(context).current()
        if (version != current.version || hash != current.yamlSha256) return
        context.getSharedPreferences("unlock-catalog", Context.MODE_PRIVATE).edit()
            .putLong("appliedVersion", version).putString("appliedHash", hash).apply()
        mutable.value = mutable.value.copy(restartRequired = false)
    }
}
