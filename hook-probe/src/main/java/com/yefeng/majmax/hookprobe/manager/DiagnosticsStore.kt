package com.yefeng.majmax.hookprobe.manager

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import androidx.core.content.FileProvider
import com.yefeng.majmax.hookprobe.BuildConfig
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.text.DateFormat
import java.util.Date
import java.util.UUID
import java.util.zip.ZipEntry
import java.util.zip.ZipOutputStream

internal data class DiagnosticRecord(
    val timeUtcMs: Long,
    val session: String,
    val seq: Long,
    val level: String,
    val component: String,
    val code: String,
    val fields: JSONObject = JSONObject(),
) {
    val displayTime: String get() = DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.MEDIUM)
        .format(Date(timeUtcMs))
    val summary: String get() = code.replace('_', ' ').lowercase().replaceFirstChar(Char::uppercase)
}

internal enum class DiagnosticKind(val filename: String) { Hook("hook"), Assistant("assistant") }

/** Private, bounded JSONL stores. The export path always passes through the same allow-list. */
internal object DiagnosticsStore {
    private const val MAX_FILE_BYTES = 1024 * 1024L
    private const val FILE_COUNT = 4
    private val hookCodes = setOf(
        "MODULE_LOADED", "NATIVE_LOADED", "NATIVE_LOAD_FAILED", "ACTIVITY_HOOK_INSTALLED",
        "ACTIVITY_HOOK_FAILED", "CONFIG_STARTED", "CONFIGURED", "CONFIG_FAILED",
        "CAPTURE_ENDPOINT_READY", "CAPTURE_ENDPOINT_LOST", "LOG_DROPPED", "HOOK_BATCH_REJECTED",
    )
    private val levels = setOf("DEBUG", "INFO", "WARN", "ERROR")
    private val components = setOf("hook.entry", "hook.native", "hook.capture", "assistant.service", "assistant.model", "assistant.engine", "manager")
    private val allowedFieldNames = setOf("api", "moduleVersion", "count", "reason")

    private fun directory(context: Context) = File(context.filesDir, "diagnostics")
    private fun logFile(context: Context, kind: DiagnosticKind, index: Int = 1) =
        File(directory(context), "${kind.filename}-$index.jsonl")

    @Synchronized
    fun appendAssistant(context: Context, level: String, component: String, code: String,
        fields: JSONObject = JSONObject()) {
        val record = DiagnosticRecord(System.currentTimeMillis(), "manager", System.nanoTime(),
            safeLevel(level), safeComponent(component), safeCode(code), safeFields(fields))
        append(context, DiagnosticKind.Assistant, record)
    }

    @Synchronized
    fun acceptHookBatch(context: Context, payload: String): Long {
        val incoming = runCatching { JSONArray(payload) }.getOrNull() ?: return 0
        if (incoming.length() > 64 || payload.toByteArray().size > 64 * 1024) return 0
        var accepted = 0L
        var highestValidated = 0L
        val known = read(context, DiagnosticKind.Hook).asSequence()
            .map { "${it.session}:${it.seq}" }.toHashSet()
        for (i in 0 until incoming.length()) {
            val source = incoming.optJSONObject(i) ?: continue
            if (source.optInt("schema", -1) != 1) continue
            val session = source.optString("session").takeIf { it.matches(Regex("[A-Fa-f0-9-]{16,64}")) } ?: continue
            val seq = source.optLong("seq", -1).takeIf { it >= 0 } ?: continue
            val code = source.optString("code")
            if (code !in hookCodes) continue
            val component = source.optString("component")
            if (component !in components) continue
            highestValidated = maxOf(highestValidated, seq)
            if ("${session}:$seq" in known) continue
            val level = source.optString("level")
            val record = DiagnosticRecord(source.optLong("timeUtcMs").takeIf { it > 0 } ?: System.currentTimeMillis(),
                session, seq, safeLevel(level), safeComponent(component), code,
                safeFields(source.optJSONObject("fields") ?: JSONObject()))
            append(context, DiagnosticKind.Hook, record)
            known += "${session}:$seq"
            accepted = maxOf(accepted, seq)
        }
        return maxOf(accepted, highestValidated)
    }

    @Synchronized
    fun read(context: Context, kind: DiagnosticKind): List<DiagnosticRecord> {
        migrateLegacy(context)
        val records = mutableListOf<DiagnosticRecord>()
        for (index in FILE_COUNT downTo 1) {
            logFile(context, kind, index).takeIf(File::isFile)?.forEachLine { line ->
                runCatching { parseAndSanitize(JSONObject(line), kind) }.getOrNull()?.let(records::add)
            }
        }
        return records.sortedWith(compareBy<DiagnosticRecord> { it.timeUtcMs }.thenBy { it.seq })
    }

    fun packageZip(context: Context, rangeMs: Long?): File {
        val now = System.currentTimeMillis()
        val hook = read(context, DiagnosticKind.Hook).filter { rangeMs == null || now - it.timeUtcMs <= rangeMs }
        val assistant = read(context, DiagnosticKind.Assistant).filter { rangeMs == null || now - it.timeUtcMs <= rangeMs }
        val shareDir = File(context.cacheDir, "diagnostics/share").apply {
            mkdirs()
            DiagnosticsStore.cleanExpiredExports(context)
        }
        val zip = File(shareDir, "diagnostics-${now}.zip")
        val manifest = JSONObject().apply {
            put("applicationId", context.packageName)
            put("appVersion", BuildConfig.VERSION_NAME)
            put("versionCode", BuildConfig.VERSION_CODE)
            put("androidApi", Build.VERSION.SDK_INT)
            put("gameVersion", runCatching {
                val packageInfo = context.packageManager.getPackageInfo(AiOverlayService.GAME, 0)
                packageInfo.versionName ?: "unknown"
            }.getOrDefault("not_installed"))
            put("exportedAtUtcMs", now)
            put("rangeMs", rangeMs ?: JSONObject.NULL)
            put("hookEntries", hook.size)
            put("assistantEntries", assistant.size)
            put("oldestUtcMs", (hook + assistant).minOfOrNull { it.timeUtcMs } ?: JSONObject.NULL)
            put("newestUtcMs", (hook + assistant).maxOfOrNull { it.timeUtcMs } ?: JSONObject.NULL)
            put("rotated", logFile(context, DiagnosticKind.Hook, 2).exists() || logFile(context, DiagnosticKind.Assistant, 2).exists())
        }
        ZipOutputStream(zip.outputStream().buffered()).use { out ->
            fun entry(name: String, text: String) {
                out.putNextEntry(ZipEntry(name)); out.write(text.toByteArray(Charsets.UTF_8)); out.closeEntry()
            }
            entry("manifest.json", manifest.toString(2))
            entry("hook.jsonl", hook.joinToString("\n", postfix = if (hook.isNotEmpty()) "\n" else "") { encode(it).toString() })
            entry("assistant.jsonl", assistant.joinToString("\n", postfix = if (assistant.isNotEmpty()) "\n" else "") { encode(it).toString() })
            entry("README.txt", "雀魂 Max Hook 诊断包。仅包含应用产生的结构化事件，不包含原始网络帧、牌局数据、账号信息或模型文件。\n")
        }
        if (zip.length() > 8 * 1024 * 1024L) {
            zip.delete()
            error("故障包超过 8 MiB，请缩短导出时间范围")
        }
        return zip
    }

    fun estimateBytes(hook: List<DiagnosticRecord>, assistant: List<DiagnosticRecord>): Long {
        fun size(records: List<DiagnosticRecord>): Long = records.sumOf {
            encode(it).toString().toByteArray(Charsets.UTF_8).size.toLong() + 1L
        }
        return size(hook) + size(assistant) + 1024L
    }

    fun cleanExpiredExports(context: Context) {
        val cutoff = System.currentTimeMillis() - 24 * 60 * 60 * 1000L
        File(context.cacheDir, "diagnostics/share").listFiles()?.filter {
            it.isFile && it.lastModified() < cutoff
        }?.forEach(File::delete)
    }

    fun shareIntent(context: Context, file: File): Intent {
        val uri: Uri = FileProvider.getUriForFile(context,
            "${context.packageName}.diagnostics", file)
        return Intent(Intent.ACTION_SEND).apply {
            type = "application/zip"
            putExtra(Intent.EXTRA_STREAM, uri)
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        }
    }

    private fun append(context: Context, kind: DiagnosticKind, record: DiagnosticRecord) {
        val dir = directory(context)
        if (!dir.exists()) dir.mkdirs()
        val file = logFile(context, kind)
        if (file.exists() && file.length() >= MAX_FILE_BYTES) {
            for (index in FILE_COUNT downTo 2) {
                val previous = logFile(context, kind, index - 1)
                val target = logFile(context, kind, index)
                target.delete()
                if (previous.exists()) previous.renameTo(target)
            }
        }
        file.appendText(encode(record).toString() + "\n", Charsets.UTF_8)
    }

    private fun encode(record: DiagnosticRecord) = JSONObject().apply {
        put("schema", 1); put("timeUtcMs", record.timeUtcMs); put("session", record.session)
        put("seq", record.seq); put("level", record.level); put("component", record.component)
        put("code", record.code); put("fields", record.fields)
    }

    private fun parseAndSanitize(value: JSONObject, kind: DiagnosticKind): DiagnosticRecord? {
        if (value.optInt("schema", -1) != 1) return null
        val code = safeCode(value.optString("code"))
        if (kind == DiagnosticKind.Hook && code !in hookCodes) return null
        return DiagnosticRecord(value.optLong("timeUtcMs").takeIf { it > 0 } ?: return null,
            value.optString("session").take(64), value.optLong("seq"), safeLevel(value.optString("level")),
            safeComponent(value.optString("component")), code,
            safeFields(value.optJSONObject("fields") ?: JSONObject()))
    }

    private fun safeFields(source: JSONObject): JSONObject = JSONObject().apply {
        for (key in allowedFieldNames) {
            if (!source.has(key)) continue
            when (val value = source.opt(key)) {
                is Number -> put(key, value)
                is String -> put(key, value.take(64).filter { it.isLetterOrDigit() || it in " _-./" })
            }
        }
    }

    private fun safeLevel(value: String) = value.takeIf { it in levels } ?: "INFO"
    private fun safeComponent(value: String) = value.takeIf { it in components } ?: "manager"
    private fun safeCode(value: String) = value.takeIf { it.matches(Regex("[A-Z0-9_]{2,48}")) } ?: "UNKNOWN_EVENT"

    @Synchronized
    private fun migrateLegacy(context: Context) {
        val legacy = File(context.filesDir, "manager-diagnostics.log")
        val marker = File(context.filesDir, "manager-diagnostics.migrated")
        if (marker.exists() || !legacy.isFile) return
        val recent = ArrayDeque<String>(2_000)
        legacy.forEachLine { line ->
            if (recent.size == 2_000) recent.removeFirst()
            recent.addLast(line)
        }
        recent.forEach { line ->
            val parts = line.split('|', limit = 4)
            if (parts.size == 4) {
                val source = parts[2].take(32).replace(Regex("[^A-Za-z0-9_.-]"), "_")
                val record = DiagnosticRecord(parts[0].toLongOrNull() ?: System.currentTimeMillis(),
                    "legacy-${UUID.randomUUID()}", 0, safeLevel(parts[1]), "manager",
                    "LEGACY_${source.uppercase().take(24)}", JSONObject().put("reason", "migrated"))
                append(context, DiagnosticKind.Assistant, record)
            }
        }
        marker.writeText("done")
        legacy.delete()
    }
}
