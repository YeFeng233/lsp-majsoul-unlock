package com.yefeng.majmax.hookprobe.manager

import android.content.Context
import android.content.ContextWrapper
import android.util.AtomicFile
import com.yefeng.majmax.hookprobe.BuildConfig
import com.yefeng.majmax.hookprobe.catalog.UnlockCatalogData
import com.yefeng.majmax.hookprobe.catalog.UnlockCatalogInstaller
import org.json.JSONObject
import java.io.ByteArrayInputStream
import java.io.File
import java.io.IOException

/** Isolated files and fake HTTPS responses: never edits real settings or contacts a proxy. */
internal object CatalogChecks {
    fun run(target: Context, tests: Context) {
        val bundled = UnlockCatalogStore.bundled(target)
        val root = File(target.cacheDir, "catalog-regressions").apply { mkdirs() }
        val isolated = object : ContextWrapper(target) {
            override fun getSharedPreferences(name: String, mode: Int) = super.getSharedPreferences("catalog-checks-$name", mode)
        }
        fun example(version: Long, mutate: (JSONObject) -> Unit = {}): Pair<String, String> {
            val data = JSONObject(bundled.payload).also(mutate).toString()
            val info = JSONObject(bundled.manifest).put("version", version).put("versionName", "test.$version")
                .put("size", data.toByteArray().size).put("sha256", UnlockCatalogData.sha256(data.toByteArray()))
            return info.toString() to data
        }
        fun parse(pair: Pair<String, String>) = UnlockCatalogData.parse(pair.first, pair.second, BuildConfig.VERSION_CODE)
        fun fetch(pair: Pair<String, String>): (String, Int) -> String = { name, _ -> if (name == "manifest.json") pair.first else pair.second }
        fun invalid(pair: Pair<String, String>) { check(runCatching { parse(pair) }.isFailure) }
        try {
            val repository = CatalogRepository(File(root, "manager"), bundled, BuildConfig.VERSION_CODE)
            check(repository.current().sha256 == bundled.sha256)
            val next = example(bundled.version + 1) { it.getJSONArray("skin").put(4_294_967_000L) }
            check(repository.update(fetch(next)))
            val installed = repository.current()
            check(installed.version == bundled.version + 1 && installed.skinCount == bundled.skinCount + 1)
            check(CatalogRepository(File(root, "manager"), bundled, BuildConfig.VERSION_CODE).current().sha256 == installed.sha256)
            var requests = 0
            check(!repository.update { name, _ -> requests++; check(name == "manifest.json"); next.first })
            check(requests == 1)
            check(!repository.update(fetch(bundled.manifest to bundled.payload))) // No rollback.
            val corrupt = example(bundled.version + 2)
            check(runCatching { repository.update(fetch(corrupt.first to "{}")) }.isFailure)
            check(repository.current().sha256 == installed.sha256)
            check(runCatching { repository.update { _, _ -> throw IOException("offline") } }.isFailure)
            check(repository.current().sha256 == installed.sha256)
            val sameVersionChanged = example(installed.version) { it.getJSONArray("skin").put(4_294_967_001L) }
            check(runCatching { repository.update(fetch(sameVersionChanged)) }.isFailure)
            val incomplete = AtomicFile(File(root, "manager/unlock-catalog.json"))
            val stream = incomplete.startWrite(); stream.write("partial".toByteArray()); incomplete.failWrite(stream)
            check(CatalogRepository(File(root, "manager"), bundled, BuildConfig.VERSION_CODE).current().sha256 == installed.sha256)
            File(root, "manager/unlock-catalog.json").writeText("broken")
            check(CatalogRepository(File(root, "manager"), bundled, BuildConfig.VERSION_CODE).current().sha256 == bundled.sha256)

            invalid(example(2) { it.remove("skin") })
            invalid(example(2) { it.put("scripts", "evil.lua") })
            invalid(example(2) { it.getJSONArray("skin").put("../ui.lua") })
            invalid(example(2) { it.getJSONArray("skin").put(-1) })
            invalid(example(2) { it.getJSONArray("skin").put(1.5) })
            invalid(example(2) { it.getJSONArray("skin").put(4_294_967_296L) })
            invalid(example(2) { it.getJSONArray("skin").put(it.getJSONArray("skin").get(0)) })
            val traversal = JSONObject(next.first).put("dataFile", "../max_data.yaml").toString()
            invalid(traversal to next.second)
            invalid(JSONObject(next.first).put("minModuleVersionCode", Int.MAX_VALUE).toString() to next.second)
            check(runCatching { UnlockCatalogData.readBounded(ByteArrayInputStream(ByteArray(33)), 32) }.isFailure)
            check(runCatching { UnlockCatalogData.utf8(byteArrayOf(0xc3.toByte(), 0x28)) }.isFailure)

            // Full-size data must stay below Binder's transaction limit when sent as UTF-8 bytes.
            val padded = bundled.payload + " ".repeat(UnlockCatalogData.MAX_DATA_BYTES - bundled.payload.toByteArray().size)
            val paddedInfo = JSONObject(bundled.manifest).put("size", padded.toByteArray().size)
                .put("sha256", UnlockCatalogData.sha256(padded.toByteArray())).toString()
            val paddedSnapshot = UnlockCatalogData.parse(paddedInfo,padded,BuildConfig.VERSION_CODE)
            val envelope = android.os.Bundle().apply {
                putString("manifest",paddedSnapshot.manifest); putByteArray("payload",paddedSnapshot.payload.toByteArray())
            }
            val parcel = android.os.Parcel.obtain()
            try { parcel.writeBundle(envelope); check(parcel.dataSize()<768*1024) } finally { parcel.recycle() }
            check(UnlockCatalogData.utf8(checkNotNull(envelope.getByteArray("payload")))==padded)

            val game = File(root, "game").apply { mkdirs() }
            val settings = File(game, "settings.mod.json").apply { writeText("{\"mainChar\":200001,\"charSkin\":{\"200001\":400102}}") }
            val original = settings.readBytes()
            check(UnlockCatalogInstaller.install(game, installed))
            check(File(game, "max_data.yaml").readText() == installed.yaml)
            check(!UnlockCatalogInstaller.install(game, bundled))
            check(File(game, "max_data.yaml").readText() == installed.yaml)
            File(game, "max_data.yaml").writeText("corrupt")
            check(UnlockCatalogInstaller.install(game, installed))
            check(File(game, "max_data.yaml").readText() == installed.yaml && settings.readBytes().contentEquals(original))

            val origin = "https://raw.githubusercontent.com/YeFeng233/lsp-majsoul-unlock/main/catalog/manifest.json"
            val route = GitHubRoute(true, "https://accelerator.example/prefix/")
            check(route.urls(origin) == listOf("https://accelerator.example/prefix/$origin", origin))
            val seen = mutableListOf<String>()
            check(GitHubConnection.requestWithFallback(route, origin) { url, _ ->
                seen.add(url); if (url != origin) throw IOException("proxy unavailable"); "{}"
            } == "{}" && seen.size == 2)
            for (bad in listOf("http://accelerator.example", "https://user:pass@accelerator.example", "https://accelerator.example/?token=a", "https://accelerator.example/#x", "")) {
                check(runCatching { GitHubRoute.validatePrefix(bad) }.isFailure)
            }
            check(!GitHubConnection.isOrigin("https://github.com.evil.example/file"))
            check(!GitHubConnection.allowedRedirect("http://github.com/file", route.prefix))
            check(!GitHubConnection.allowedRedirect("https://unrelated.example/file", route.prefix))
            check(GitHubConnection.allowedRedirect("https://accelerator.example/cache/file", route.prefix))
            check(GitHubConnection.allowedRedirect("https://release-assets.githubusercontent.com/file", null))
            GitHubConnection.save(isolated, true, "https://accelerator.example")
            check(GitHubConnection.settings(isolated).accelerated)
            GitHubConnection.save(isolated, false, "bad address")
            check(!GitHubConnection.settings(isolated).accelerated)
            println("Unlock catalog cache, recovery, game installation and HTTPS route checks passed")
        } finally {
            isolated.getSharedPreferences("github-connection", Context.MODE_PRIVATE).edit().clear().commit()
            root.deleteRecursively()
        }
    }
}
