package com.yefeng.majmax.hookprobe.manager

import android.content.Context
import com.yefeng.majmax.hookprobe.BuildConfig
import com.yefeng.majmax.hookprobe.catalog.UnlockCatalogData
import org.json.JSONArray
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URI
import java.net.URL

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
    fun settings(context: Context): GitHubRoute {
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val enabled = prefs.getBoolean("accelerated", false)
        val prefix = prefs.getString("prefix", "") ?: ""
        return if (enabled && runCatching { GitHubRoute.validatePrefix(prefix) }.isSuccess) GitHubRoute(true, prefix)
            else GitHubRoute(false, prefix)
    }

    fun save(context: Context, enabled: Boolean, prefix: String) {
        require(!enabled || prefix.isNotBlank()) { "请先填写加速地址" }
        val address = if (enabled) GitHubRoute.validatePrefix(prefix)
            else runCatching { GitHubRoute.validatePrefix(prefix) }.getOrDefault("")
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
            .putBoolean("accelerated", enabled).putString("prefix", address).apply()
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

    internal fun requestWithFallback(route: GitHubRoute, origin: String, read: (String, String?) -> String): String {
        var failure: Exception? = null
        for (url in route.urls(origin)) {
            try {
                return read(url, if (url == origin) null else route.prefix)
            } catch (error: Exception) { failure = error }
        }
        throw IllegalStateException("无法连接 GitHub 更新源，请检查网络或加速设置", failure)
    }

    private fun read(url: String, accept: String, limit: Int, proxy: String?): String {
        var next = url
        repeat(6) {
            require(allowedRedirect(next, proxy)) { "更新源跳转到不受支持的地址" }
            val connection = (URL(next).openConnection() as HttpURLConnection).apply {
                connectTimeout = 8_000; readTimeout = 8_000
                instanceFollowRedirects = false
                setRequestProperty("Accept", accept)
                setRequestProperty("User-Agent", "MajsoulMax-Hook/${BuildConfig.VERSION_NAME}")
            }
            try {
                val code = connection.responseCode
                if (code in listOf(301, 302, 303, 307, 308)) {
                    val location = connection.getHeaderField("Location") ?: error("更新源跳转地址为空")
                    next = URI(next).resolve(location).toASCIIString()
                } else {
                    check(code in 200..299) { "更新源返回 HTTP $code" }
                    val length = connection.contentLengthLong
                    require(length <= limit) { "下载内容超过大小限制" }
                    return connection.inputStream.use { UnlockCatalogData.utf8(UnlockCatalogData.readBounded(it, limit)) }
                }
            } finally { connection.disconnect() }
        }
        error("更新源跳转次数过多")
    }

    fun downloadUrl(context: Context, origin: String): String = settings(context).urls(origin).first()
}
