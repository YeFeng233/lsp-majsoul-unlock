package com.yefeng.majmax.hookprobe.manager

import android.content.Context
import android.os.SystemClock
import com.yefeng.majmax.hookprobe.BuildConfig
import com.yefeng.majmax.hookprobe.catalog.UnlockCatalogData
import org.json.JSONArray
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URI
import java.net.URL
import java.io.ByteArrayOutputStream
import java.io.File
import kotlinx.coroutines.CancellationException

internal data class GitHubRoute(val accelerated: Boolean = false, val prefix: String = "") {
    fun urls(origin: String): List<String> {
        require(GitHubConnection.isOrigin(origin)) { "不是受支持的 GitHub 地址" }
        return if (accelerated) listOf(validatePrefix(prefix) + origin, origin) else listOf(origin)
    }

    companion object {
        fun validatePrefix(value: String): String {
            val uri = URI(value.trim())
            require(uri.scheme == "https" && !uri.host.isNullOrBlank() && uri.rawUserInfo == null &&
                uri.rawQuery == null && uri.rawFragment == null && uri.port in -1..65535 && uri.port != 0) {
                "加速地址需为不含账号、查询参数或片段的 HTTPS 地址"
            }
            return uri.toASCIIString().trimEnd('/') + "/"
        }
    }
}

internal object GitHubConnection {
    private const val PREFS = "github-connection"
    data class Preset(val name: String, val prefix: String)
    val presets = listOf(
        Preset("GH-Proxy · gh-proxy.com", "https://gh-proxy.com/"),
        Preset("LLKK · gh.llkk.cc", "https://gh.llkk.cc/"),
        Preset("GHFast · ghfast.top", "https://ghfast.top/"),
        Preset("GH-Proxy 备用 · gh-proxy.org", "https://gh-proxy.org/"),
        Preset("iSteed · cors.isteed.cc", "https://cors.isteed.cc/"),
    )

    fun customPrefix(context: Context): String {
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        return prefs.getString("custom-prefix", null)
            ?: settings(context).prefix.takeUnless { value -> presets.any { it.prefix == value } }.orEmpty()
    }
    fun settings(context: Context): GitHubRoute {
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val enabled = prefs.getBoolean("accelerated", false)
        val prefix = prefs.getString("prefix", "") ?: ""
        return if (enabled && runCatching { GitHubRoute.validatePrefix(prefix) }.isSuccess) GitHubRoute(true, prefix)
            else GitHubRoute(false, prefix)
    }

    fun save(context: Context, enabled: Boolean, prefix: String, customPrefix: String? = null) {
        require(!enabled || prefix.isNotBlank()) { "请先填写加速地址" }
        val address = if (enabled) GitHubRoute.validatePrefix(prefix)
            else runCatching { GitHubRoute.validatePrefix(prefix) }.getOrDefault("")
        val editor = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
            .putBoolean("accelerated", enabled).putString("prefix", address)
        if (customPrefix != null) editor.putString("custom-prefix", customPrefix.trim())
        else if (address.isNotBlank() && presets.none { it.prefix == address }) editor.putString("custom-prefix", address)
        editor.apply()
    }

    fun isOrigin(value: String): Boolean = runCatching {
        val uri = URI(value)
        val host = uri.host?.lowercase() ?: return false
        uri.scheme == "https" && uri.rawUserInfo == null && uri.rawFragment == null &&
            uri.port in listOf(-1, 443) &&
            (host == "github.com" || host == "api.github.com" || host.endsWith(".githubusercontent.com"))
    }.getOrDefault(false)

    internal fun allowedRedirect(value: String, proxy: String?): Boolean {
        if (isOrigin(value)) return true
        return runCatching {
            val target = URI(value)
            val custom = proxy?.let(::URI) ?: return false
            target.scheme == "https" && target.rawUserInfo == null && target.rawFragment == null &&
                target.host.equals(custom.host, ignoreCase = true) && target.port == custom.port
        }.getOrDefault(false)
    }

    /** Public GitHub JSON only. A broken proxy falls back to the original HTTPS URL. */
    fun requestJson(context: Context, origin: String, accept: String = "application/json", limit: Int = 2 * 1024 * 1024): String {
        val route = settings(context)
        return requestWithFallback(route, origin) { url, proxy ->
            val text = read(url, accept, limit, proxy)
            when (text.trimStart().firstOrNull()) {
                '{' -> JSONObject(text)
                '[' -> JSONArray(text)
                else -> error("下载内容不是 JSON 清单")
            }
            text
        }
    }

    internal fun <T> requestWithFallback(route: GitHubRoute, origin: String, read: (String, String?) -> T): T {
        var failure: Exception? = null
        for (url in route.urls(origin)) {
            try {
                return read(url, if (url == origin) null else route.prefix)
            } catch (error: CancellationException) { throw error }
            catch (error: Exception) { failure = error }
        }
        throw IllegalStateException("无法连接 GitHub 更新源，请检查网络或加速设置", failure)
    }

    /** A speed probe uses only the nominated source, never the direct fallback. */
    internal fun probeJson(url: String, proxy: String): String =
        read(url, "application/json", UnlockCatalogData.MAX_MANIFEST_BYTES, proxy, SystemClock.elapsedRealtime() + 8_000)

    private fun read(url: String, accept: String, limit: Int, proxy: String?, deadline: Long? = null): String {
        fun remaining(): Int = if (deadline == null) 8_000 else
            (deadline - SystemClock.elapsedRealtime()).also {
                if (it <= 0) throw java.net.SocketTimeoutException("请求超时")
            }.coerceAtMost(8_000).toInt()
        var next = url
        repeat(6) {
            require(allowedRedirect(next, proxy)) { "更新源跳转到不受支持的地址" }
            val connection = (URL(next).openConnection() as HttpURLConnection).apply {
                connectTimeout = remaining(); readTimeout = remaining()
                instanceFollowRedirects = false
                if (deadline != null) useCaches = false
                setRequestProperty("Accept", accept)
                setRequestProperty("User-Agent", "MajsoulMax-Hook/${BuildConfig.VERSION_NAME}")
            }
            try {
                val code = connection.responseCode
                if (deadline != null) remaining()
                if (code in listOf(301, 302, 303, 307, 308)) {
                    val location = connection.getHeaderField("Location") ?: error("更新源跳转地址为空")
                    next = URI(next).resolve(location).toASCIIString()
                } else {
                    check(code in 200..299) { "更新源返回 HTTP $code" }
                    val length = connection.contentLengthLong
                    require(length <= limit) { "下载内容超过大小限制" }
                    return connection.inputStream.use { input ->
                        if (deadline == null) UnlockCatalogData.utf8(UnlockCatalogData.readBounded(input, limit))
                        else {
                            val bytes = ByteArrayOutputStream()
                            val buffer = ByteArray(4096)
                            while (true) {
                                connection.readTimeout = remaining()
                                val count = input.read(buffer)
                                remaining()
                                if (count == -1) break
                                require(bytes.size() + count <= limit) { "下载内容超过大小限制" }
                                bytes.write(buffer, 0, count)
                            }
                            UnlockCatalogData.utf8(bytes.toByteArray())
                        }
                    }
                }
            } finally { connection.disconnect() }
        }
        error("更新源跳转次数过多")
    }

    fun downloadUrl(context: Context, origin: String): String = settings(context).urls(origin).first()

    /** Stream APK bytes through the same selected route and direct fallback as catalogs. */
    fun downloadApk(context: Context, origin: String, target: File,
        progress: (Long, Long) -> Unit, validate: (File) -> Unit) {
        requestWithFallback(settings(context), origin) { url, proxy ->
            // Each attempt replaces the partial file; bytes from different routes never mix.
            target.delete()
            streamApk(url, proxy, target, progress)
            validate(target)
        }
    }

    private fun streamApk(url: String, proxy: String?, target: File, progress: (Long, Long) -> Unit) {
        val limit = 512L * 1024 * 1024
        var next = url
        repeat(6) {
            progress(0, 0)
            require(allowedRedirect(next, proxy)) { "安装包跳转到不受支持的地址" }
            val connection = (URL(next).openConnection() as HttpURLConnection).apply {
                connectTimeout = 15_000; readTimeout = 15_000
                instanceFollowRedirects = false
                setRequestProperty("Accept", "application/octet-stream")
                setRequestProperty("Accept-Encoding", "identity")
                setRequestProperty("User-Agent", "MajsoulMax-Hook/${BuildConfig.VERSION_NAME}")
            }
            try {
                val code = connection.responseCode
                if (code in listOf(301, 302, 303, 307, 308)) {
                    next = URI(next).resolve(connection.getHeaderField("Location") ?: error("安装包跳转地址为空")).toASCIIString()
                } else {
                    check(code == 200) { "安装包下载返回 HTTP $code" }
                    val total = connection.contentLengthLong
                    require(total <= limit) { "安装包超过 512 MiB 限制" }
                    var bytes = 0L
                    connection.inputStream.use { input ->
                        target.outputStream().buffered().use { output ->
                            val buffer = ByteArray(64 * 1024)
                            while (true) {
                                val count = input.read(buffer)
                                if (count < 0) break
                                bytes += count
                                require(bytes <= limit) { "安装包超过 512 MiB 限制" }
                                progress(bytes, total.coerceAtLeast(0))
                                output.write(buffer, 0, count)
                            }
                        }
                    }
                    check(bytes > 0 && (total < 0 || total == bytes)) { "安装包下载不完整，请重试" }
                    return
                }
            } finally { connection.disconnect() }
        }
        error("安装包跳转次数过多")
    }
}
