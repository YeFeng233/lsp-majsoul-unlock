package com.yefeng.majmax.hookprobe.manager

import android.content.Context
import android.content.pm.ApplicationInfo
import android.content.pm.PackageInfo
import android.os.Build
import java.io.File
import java.io.IOException
import java.util.UUID
import kotlinx.coroutines.CancellationException

internal object ApkUpdateChecks {
    fun run(context: Context) {
        val root = File(context.cacheDir, "apk-checks-${UUID.randomUUID()}").apply { mkdirs() }
        try {
            val archive = PackageInfo().apply {
                packageName = context.packageName
                longVersionCode = 101
                applicationInfo = ApplicationInfo().apply { minSdkVersion = 29 }
            }
            ApkUpdates.validateIdentity(context.packageName, 100, archive, 101)
            fun rejected() = check(runCatching {
                ApkUpdates.validateIdentity(context.packageName, 100, archive, 101)
            }.isFailure)
            archive.packageName = "another.application"; rejected()
            archive.packageName = context.packageName
            archive.longVersionCode = 100; rejected()
            archive.longVersionCode = 102; rejected()
            archive.longVersionCode = 101
            archive.applicationInfo!!.minSdkVersion = Build.VERSION.SDK_INT + 1; rejected()

            val origin = "https://github.com/owner/repo/releases/download/v1/update.apk"
            val routes = mutableListOf<String>()
            val apk = File(root, "download.apk")
            // APKs use the same generic route helper as JSON; verify typed results and fallback.
            val result = GitHubConnection.requestWithFallback(GitHubRoute(true, "https://proxy.example/"), origin) { url, _ ->
                routes += url
                if (url != origin) throw IOException("proxy unavailable")
                apk.apply { writeText("not an APK") }
            }
            check(result == apk && routes == listOf("https://proxy.example/$origin", origin))
            var attempts = 0
            check(runCatching {
                GitHubConnection.requestWithFallback(GitHubRoute(true, "https://proxy.example/"), origin) { _, _ ->
                    attempts++
                    throw CancellationException("cancelled")
                }
            }.exceptionOrNull() is CancellationException)
            check(attempts == 1)

            val release = ApkRelease(101, origin)
            check(runCatching { ApkUpdates.verify(context, apk, release) }.isFailure)
            check(runCatching { ApkUpdates.verify(context, apk, release.copy(bytes = apk.length() + 1)) }
                .exceptionOrNull()?.message == "安装包大小与发布记录不一致")
            check(runCatching { ApkUpdates.verify(context, apk, release.copy(sha256 = "0".repeat(64))) }
                .exceptionOrNull()?.message == "安装包校验失败，请重新下载")
        } finally {
            root.deleteRecursively()
        }
    }
}
