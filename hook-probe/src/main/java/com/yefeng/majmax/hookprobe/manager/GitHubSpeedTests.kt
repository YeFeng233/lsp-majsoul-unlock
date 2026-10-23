package com.yefeng.majmax.hookprobe.manager

import android.content.Context
import android.os.SystemClock
import com.yefeng.majmax.hookprobe.BuildConfig
import com.yefeng.majmax.hookprobe.catalog.UnlockCatalogData
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withPermit

internal data class GitHubSpeedResult(
    val source: GitHubConnection.Preset, val latencyMs: Long? = null, val error: String? = null,
)

internal data class GitHubSpeedStatus(
    val testing: Boolean = false, val results: List<GitHubSpeedResult> = emptyList(),
    val message: String = "", val failed: Boolean = false, val winner: String? = null,
    val completedAt: Long = 0, val cooldownUntil: Long = 0,
)

/** Application-scoped: navigating away or rotating must not lose a result or launch duplicate probes. */
internal object GitHubSpeedTests {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val mutable = MutableStateFlow(GitHubSpeedStatus())
    val state = mutable.asStateFlow()

    internal fun candidates(customPrefix: String): List<GitHubConnection.Preset> {
        val custom = runCatching { GitHubRoute.validatePrefix(customPrefix) }.getOrNull()
        return (GitHubConnection.presets + listOfNotNull(custom?.let { GitHubConnection.Preset("自定义地址", it) }))
            .distinctBy { it.prefix }
    }

    internal fun measure(
        source: GitHubConnection.Preset, origin: String, current: UnlockCatalogData.Snapshot,
        clock: () -> Long = SystemClock::elapsedRealtime,
        fetch: (String, String) -> String = GitHubConnection::probeJson,
    ): GitHubSpeedResult {
        val started = clock()
        return try {
            require(GitHubConnection.isOrigin(origin)) { "测速文件地址无效" }
            val prefix = GitHubRoute.validatePrefix(source.prefix)
            val body = fetch(prefix + origin, prefix)
            val info = UnlockCatalogData.manifest(body, BuildConfig.VERSION_CODE)
            require(info.getLong("version") >= current.version) { "线路缓存的清单版本过旧" }
            require(info.getLong("version") != current.version || info.getString("sha256") == current.sha256) { "清单校验值不一致" }
            GitHubSpeedResult(source, (clock() - started).coerceAtLeast(1))
        } catch (error: CancellationException) { throw error }
        catch (error: Exception) {
            val message = when (error) {
                is java.net.SocketTimeoutException -> "请求超时"
                is java.net.UnknownHostException -> "域名无法解析"
                is javax.net.ssl.SSLException -> "HTTPS 连接失败"
                is java.net.ConnectException -> "连接失败"
                is org.json.JSONException -> "返回内容不是有效清单"
                else -> error.message?.take(70) ?: "请求失败"
            }
            GitHubSpeedResult(source, error = message)
        }
    }

    internal fun fastest(results: List<GitHubSpeedResult>): GitHubSpeedResult? =
        results.filter { it.error == null && (it.latencyMs ?: 0) > 0 }.minByOrNull { it.latencyMs!! }

    internal fun selectFastest(context: Context, results: List<GitHubSpeedResult>, custom: String): GitHubSpeedResult? {
        val winner = fastest(results) ?: return null
        GitHubConnection.save(context, true, winner.source.prefix, custom)
        return winner
    }

    @Synchronized fun start(context: Context, custom: String) {
        val now = SystemClock.elapsedRealtime()
        if (mutable.value.testing || now < mutable.value.cooldownUntil) return
        val app = context.applicationContext
        val sources = candidates(custom)
        mutable.value = GitHubSpeedStatus(testing = true, results = sources.map { GitHubSpeedResult(it) },
            message = "正在测试各加速源…", cooldownUntil = now + 30_000)
        scope.launch {
            try {
                val current = UnlockCatalogStore.repo(app).current()
                val origin = UnlockCatalogStore.baseUrl() + "manifest.json?probe=" + System.currentTimeMillis()
                val semaphore = Semaphore(3)
                val results = coroutineScope {
                    sources.map { source -> async {
                        semaphore.withPermit {
                            measure(source, origin, current).also { result ->
                                mutable.update { previous -> previous.copy(results = previous.results.map {
                                    if (it.source.prefix == source.prefix) result else it
                                }) }
                            }
                        }
                    } }.awaitAll()
                }
                val winner = selectFastest(app, results, custom)
                val skipped = if (custom.isNotBlank() && runCatching { GitHubRoute.validatePrefix(custom) }.isFailure)
                    "；无效自定义地址已跳过" else ""
                mutable.update { it.copy(testing = false, results = results, winner = winner?.source?.prefix,
                    completedAt = System.currentTimeMillis(), failed = winner == null,
                    message = (if (winner == null) "全部线路测试失败，已保留原连接设置"
                        else "已自动启用并保存：${winner.source.name}（${winner.latencyMs} ms）") + skipped) }
            } catch (error: Exception) {
                mutable.update { it.copy(testing = false, completedAt = System.currentTimeMillis(), failed = true,
                    results = it.results.map { result -> if (result.latencyMs == null && result.error == null)
                        result.copy(error = "测试中断") else result },
                    message = "测速未完成，已保留原连接设置") }
            }
        }
    }
}
