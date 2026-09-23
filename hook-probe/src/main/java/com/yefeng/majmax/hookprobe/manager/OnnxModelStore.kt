package com.yefeng.majmax.hookprobe.manager

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import ai.onnxruntime.OnnxJavaType
import ai.onnxruntime.OnnxTensor
import ai.onnxruntime.OrtEnvironment
import ai.onnxruntime.OrtSession
import ai.onnxruntime.TensorInfo
import java.io.File
import java.nio.FloatBuffer
import java.security.MessageDigest
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.UUID

internal data class ImportedPolicy(val players: Int, val name: String, val bytes: Long, val sha256: String)

/** Validates and runs only the documented Akagi policy contract, inside app-private storage. */
internal object OnnxModelStore {
    private const val MAX_BYTES = 128L * 1024 * 1024
    private const val PREFS = "onnx-policy-models"
    private val environment by lazy { OrtEnvironment.getEnvironment() }
    private val timeoutScheduler = Executors.newSingleThreadScheduledExecutor { task ->
        Thread(task, "majmax-onnx-timeout").apply { isDaemon = true }
    }
    private data class Cached(val file: File, val session: OrtSession)
    private val sessions = mutableMapOf<String, Cached>()
    private val activeSessions = mutableMapOf<Int, Cached>()
    private val failedPlayers = mutableSetOf<Int>()
    @Volatile private var appContext: Context? = null

    fun initialize(context: Context) { appContext = context.applicationContext }

    fun info(context: Context, players: Int): ImportedPolicy? {
        require(players == 3 || players == 4)
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val path = selectedFile(context, players)
        if (!path.isFile) return null
        return ImportedPolicy(players, prefs.getString("name_$players", path.name) ?: path.name,
            path.length(), prefs.getString("sha_$players", "") ?: "")
    }

    fun selectedCustom(context: Context, players: Int): Boolean =
        info(context, players) != null && context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .getBoolean("enabled_$players", false)

    fun enabledMask(context: Context): Int =
        (if (selectedCustom(context, 4)) 1 else 0) or (if (selectedCustom(context, 3)) 2 else 0)

    fun prepareSelected(context: Context) {
        initialize(context)
        for (players in listOf(4, 3)) {
            if (!selectedCustom(context, players)) {
                synchronized(sessions) { activeSessions.remove(players) }
                continue
            }
            val result = activate(players)
            if (result != 0) {
                context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
                    .putBoolean("enabled_$players", false).apply()
                DiagnosticsStore.appendAssistant(context, "ERROR", "assistant.model", "MODEL_STARTUP_FALLBACK",
                    org.json.JSONObject().put("count", players))
            }
        }
    }

    fun selectBuiltIn(context: Context, players: Int) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putBoolean("enabled_$players", false).apply()
        DiagnosticsStore.appendAssistant(context, "INFO", "assistant.model", "MODEL_BUILTIN",
            org.json.JSONObject().put("count", players))
    }

    fun selectCustom(context: Context, players: Int) {
        check(info(context, players) != null) { "请先导入有效的 ONNX 模型" }
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putBoolean("enabled_$players", true).apply()
        DiagnosticsStore.appendAssistant(context, "INFO", "assistant.model", "MODEL_CUSTOM_SELECTED",
            org.json.JSONObject().put("count", players))
    }

    fun delete(context: Context, players: Int) {
        val oldFile = selectedFile(context, players)
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
            .remove("enabled_$players").remove("name_$players").remove("sha_$players").remove("file_$players").apply()
        synchronized(sessions) {
            if (sessions.values.none { it.file.absolutePath == oldFile.absolutePath }) oldFile.delete()
        }
        DiagnosticsStore.appendAssistant(context, "INFO", "assistant.model", "MODEL_DELETED",
            org.json.JSONObject().put("count", players))
    }

    fun import(context: Context, uri: Uri): ImportedPolicy {
        initialize(context)
        val displayName = context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
            ?.use { cursor -> if (cursor.moveToFirst()) cursor.getString(0) else null }
            ?: "policy.onnx"
        require(displayName.endsWith(".onnx", ignoreCase = true)) { "请选择 .onnx 文件" }
        val parent = File(context.filesDir, "models/incoming/${UUID.randomUUID()}").apply { mkdirs() }
        val staged = File(parent, "policy.onnx")
        val digest = MessageDigest.getInstance("SHA-256")
        var bytes = 0L
        try {
            val input = checkNotNull(context.contentResolver.openInputStream(uri)) { "无法读取所选文件" }
            input.use { source -> staged.outputStream().buffered().use { output ->
                val buffer = ByteArray(64 * 1024)
                while (true) {
                    val count = source.read(buffer)
                    if (count < 0) break
                    bytes += count
                    require(bytes <= MAX_BYTES) { "模型文件超过 128 MiB 限制" }
                    digest.update(buffer, 0, count)
                    output.write(buffer, 0, count)
                }
            } }
            require(bytes > 0) { "模型文件为空" }
            rejectExternalWeights(staged)
            val validation = validate(staged)
            val players = validation.first
            val hash = digest.digest().joinToString("") { "%02x".format(it) }
            val folder = File(File(context.filesDir, "models"), "$players-$hash").apply { mkdirs() }
            val target = File(folder, "policy.onnx")
            val temp = File(folder, "policy.tmp")
            staged.copyTo(temp, overwrite = true)
            if (target.exists()) temp.delete() else check(temp.renameTo(target)) { "无法保存模型文件" }
            synchronized(sessions) {
                if (target.absolutePath !in sessions) {
                    sessions[target.absolutePath] = Cached(target, createValidatedSession(target, players))
                }
            }
            context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
                .putString("name_$players", validation.second.ifBlank { displayName }.take(64))
                .putString("sha_$players", hash)
                .putString("file_$players", target.relativeTo(context.filesDir).path)
                .putBoolean("enabled_$players", true)
                .apply()
            DiagnosticsStore.appendAssistant(context, "INFO", "assistant.model", "MODEL_IMPORTED",
                org.json.JSONObject().put("count", players).put("reason", hash.take(12)))
            return ImportedPolicy(players, validation.second.ifBlank { displayName }.take(64), bytes, hash)
        } catch (error: Throwable) {
            DiagnosticsStore.appendAssistant(context, "ERROR", "assistant.model", "MODEL_IMPORT_REJECTED")
            throw error
        } finally {
            staged.delete()
            parent.delete()
        }
    }

    /** JNI calls this on the assistant worker thread. A null result means use the bundled Candle model. */
    @JvmStatic
    fun infer(players: Int, observation: FloatArray): FloatArray? {
        val context = appContext ?: return null
        if (players != 3 && players != 4) return null
        return try {
            synchronized(sessions) {
                if (players in failedPlayers) return null
                val cached = activeSessions[players] ?: return null
                run(cached.session, players, observation)
            }
        } catch (error: Throwable) {
            DiagnosticsStore.appendAssistant(context, "ERROR", "assistant.model", "MODEL_INFERENCE_FAILED",
                org.json.JSONObject().put("count", players))
            synchronized(sessions) { failedPlayers += players; activeSessions.remove(players) }
            null
        }
    }

    /** Called once by Rust when a complete new game is synchronized. */
    @JvmStatic
    fun activate(players: Int): Int {
        val context = appContext ?: return -1
        if (players != 3 && players != 4 || !selectedCustom(context, players)) return -1
        return try {
            synchronized(sessions) {
                val file = selectedFile(context, players)
                check(file.isFile) { "模型文件不存在" }
                val cached = sessions[file.absolutePath] ?: Cached(file, createValidatedSession(file, players))
                    .also { sessions[file.absolutePath] = it }
                activeSessions[players] = cached
                failedPlayers.remove(players)
            }
            0
        } catch (error: Throwable) {
            DiagnosticsStore.appendAssistant(context, "ERROR", "assistant.model", "MODEL_ACTIVATION_FAILED",
                org.json.JSONObject().put("count", players))
            -1
        }
    }

    fun selfTest(context: Context, players: Int): String {
        val started = System.nanoTime()
        val file = selectedFile(context, players)
        check(file.isFile) { "尚未导入${players}人模型" }
        val session = createValidatedSession(file, players)
        session.use { run(it, players, FloatArray(inputSize(players))) }
        return "${players}人模型可用 · ${TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - started)} ms"
    }

    private fun validate(file: File): Pair<Int, String> {
        val session = createSession(file)
        session.use {
            val metadata = session.metadata
            require(metadata.getCustomMetadataValue("majmax.contract").orElse("") == "akagi-policy-v1") { "模型协议不匹配" }
            val players = metadata.getCustomMetadataValue("majmax.players").orElse("").toIntOrNull()
                ?: throw IllegalArgumentException("模型未声明适用人数")
            require(players == 3 || players == 4) { "模型人数仅支持三麻或四麻" }
            require(metadata.getCustomMetadataValue("majmax.obs_schema").orElse("") == "1") { "特征协议版本不匹配" }
            require(metadata.getCustomMetadataValue("majmax.action_codec").orElse("") == "riichienv-core-0.4.8") { "动作编号协议不匹配" }
            require(session.inputNames == setOf("obs") && session.outputNames == setOf("logits")) { "输入/输出节点名不匹配" }
            require(tensorInfo(session.inputInfo.getValue("obs")).let {
                it.type == OnnxJavaType.FLOAT && it.shape.contentEquals(inputShape(players))
            }) { "输入节点必须是协议规定的 float32 形状" }
            require(tensorInfo(session.outputInfo.getValue("logits")).let {
                it.type == OnnxJavaType.FLOAT && it.shape.contentEquals(outputShape(players))
            }) { "输出节点必须是协议规定的 float32 形状" }
            run(session, players, FloatArray(inputSize(players)))
            return players to metadata.description
        }
    }

    private fun createValidatedSession(file: File, players: Int): OrtSession {
        val session = createSession(file)
        try {
            val metadata = session.metadata
            require(metadata.getCustomMetadataValue("majmax.contract").orElse("") == "akagi-policy-v1")
            require(metadata.getCustomMetadataValue("majmax.players").orElse("").toIntOrNull() == players)
            require(metadata.getCustomMetadataValue("majmax.obs_schema").orElse("") == "1")
            require(metadata.getCustomMetadataValue("majmax.action_codec").orElse("") == "riichienv-core-0.4.8")
            require(session.inputNames == setOf("obs") && session.outputNames == setOf("logits"))
            require(tensorInfo(session.inputInfo.getValue("obs")).let { it.type == OnnxJavaType.FLOAT && it.shape.contentEquals(inputShape(players)) })
            require(tensorInfo(session.outputInfo.getValue("logits")).let { it.type == OnnxJavaType.FLOAT && it.shape.contentEquals(outputShape(players)) })
            run(session, players, FloatArray(inputSize(players)))
            return session
        } catch (error: Throwable) {
            session.close()
            throw error
        }
    }

    private fun run(session: OrtSession, players: Int, observation: FloatArray): FloatArray {
        require(observation.size == inputSize(players))
        require(observation.all(Float::isFinite)) { "观察特征无效" }
        val tensor = OnnxTensor.createTensor(environment, FloatBuffer.wrap(observation), inputShape(players))
        val options = OrtSession.RunOptions()
        val timeout = timeoutScheduler.schedule({ runCatching { options.setTerminate(true) } }, 500, TimeUnit.MILLISECONDS)
        try {
            tensor.use {
                session.run(mapOf("obs" to it), setOf("logits"), options).use { results ->
                    val output = results.get("logits").orElseThrow() as OnnxTensor
                    val values = output.floatBuffer
                    val logits = FloatArray(values.remaining()).also { values.get(it) }
                    require(logits.size == outputSize(players) && logits.all(Float::isFinite)) { "模型输出无效" }
                    return logits
                }
            }
        } finally {
            timeout.cancel(false)
            options.close()
        }
    }

    private fun tensorInfo(info: ai.onnxruntime.NodeInfo): TensorInfo = info.info as? TensorInfo
        ?: throw IllegalArgumentException("只支持稠密张量模型")

    /** TensorProto.external_data uses field 13 containing a StringStringEntry key named location. */
    private fun rejectExternalWeights(file: File) {
        val bytes = file.readBytes()
        val key = "location".toByteArray(Charsets.US_ASCII)
        for (start in 4..bytes.size - key.size) {
            var matches = true
            for (offset in key.indices) if (bytes[start + offset] != key[offset]) { matches = false; break }
            if (matches && bytes[start - 2] == 0x0a.toByte() && bytes[start - 1] == key.size.toByte() &&
                bytes[start - 4] == 0x6a.toByte()) {
                throw IllegalArgumentException("不支持引用外置权重文件的 ONNX 模型")
            }
        }
    }

    private fun createSession(file: File): OrtSession {
        val options = OrtSession.SessionOptions()
        return try { environment.createSession(file.absolutePath, options) }
        finally { options.close() }
    }

    fun closeAll() = synchronized(sessions) {
        sessions.values.forEach { runCatching { it.session.close() } }
        sessions.clear(); activeSessions.clear(); failedPlayers.clear()
    }
    private fun selectedFile(context: Context, players: Int): File {
        val relative = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString("file_$players", null)
        return relative?.let { File(context.filesDir, it) }
            ?: File(File(context.filesDir, "models"), "policy-$players.onnx")
    }
    private fun inputSize(players: Int) = if (players == 4) 39 * 34 else 37 * 27
    private fun outputSize(players: Int) = if (players == 4) 82 else 60
    private fun inputShape(players: Int) = if (players == 4) longArrayOf(1, 39, 34) else longArrayOf(1, 37, 27)
    private fun outputShape(players: Int) = if (players == 4) longArrayOf(1, 82) else longArrayOf(1, 60)
}
