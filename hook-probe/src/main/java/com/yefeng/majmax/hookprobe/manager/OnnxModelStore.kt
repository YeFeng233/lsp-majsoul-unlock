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

internal data class ImportedPolicy(
    val players: Int,
    val name: String,
    val bytes: Long,
    val sha256: String,
    val format: String = "akagi-onnx",
)

/**
 * Validates and runs Akagi (.onnx) and Mortal (.pth / .onnx) policy models
 * in app-private storage. Mortal v4 supports four players and the 775-channel sanma contract.
 */
internal object OnnxModelStore {
    const val MAX_MODEL_MIB = 300
    private const val MAX_BYTES = MAX_MODEL_MIB * 1024L * 1024
    private const val PREFS = "onnx-policy-models"
    private val environment by lazy { OrtEnvironment.getEnvironment() }
    private val timeoutScheduler = Executors.newSingleThreadScheduledExecutor { task ->
        Thread(task, "majmax-onnx-timeout").apply { isDaemon = true }
    }

    internal data class OnnxSpec(
        val players: Int,
        val description: String,
        val contractKind: Int, // 0 = Akagi ONNX, 1 = Mortal ONNX
        val obsVersion: Int,
        val obsChannels: Int,
        val tileDim: Int,
        val actionDim: Int,
        /** Actual graph tensor names, resolved from the session (not hard-coded). */
        val obsInputName: String = "obs",
        val actionOutputName: String = "logits",
        /**
         * Optional Mortal legal-action mask, supplied from the current Rust legal actions.
         */
        val auxInputs: Map<String, AuxInput> = emptyMap(),
    )

    internal data class AuxInput(val type: OnnxJavaType, val shape: LongArray)

    private data class Cached(val file: File, val session: OrtSession, val spec: OnnxSpec)
    private val sessions = mutableMapOf<String, Cached>()
    private val activeSessions = mutableMapOf<Int, Cached>()
    private val failedPlayers = mutableSetOf<Int>()

    /** Akagi 4P/3P and experimental Mortal v4 4P/sanma. */
    private val KNOWN_ACTION_SPACES = setOf(82, 60, 46, 44)
    @Volatile private var appContext: Context? = null

    fun initialize(context: Context) { appContext = context.applicationContext }

    fun info(context: Context, players: Int): ImportedPolicy? {
        require(players == 3 || players == 4)
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val path = selectedFile(context, players)
        if (!path.isFile) return null
        val defaultFormat = if (path.name.endsWith(".pth", ignoreCase = true) || path.name.endsWith(".pt", ignoreCase = true)) {
            "mortal-pth"
        } else {
            "akagi-onnx"
        }
        return ImportedPolicy(
            players = players,
            name = prefs.getString("name_$players", path.name) ?: path.name,
            bytes = path.length(),
            sha256 = prefs.getString("sha_$players", "") ?: "",
            format = prefs.getString("format_$players", defaultFormat) ?: defaultFormat,
        )
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
        check(info(context, players) != null) { "请先导入有效的 .onnx 或 .pth 模型" }
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putBoolean("enabled_$players", true).apply()
        DiagnosticsStore.appendAssistant(context, "INFO", "assistant.model", "MODEL_CUSTOM_SELECTED",
            org.json.JSONObject().put("count", players))
    }

    fun delete(context: Context, players: Int) {
        val oldFile = selectedFile(context, players)
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
            .remove("enabled_$players")
            .remove("name_$players")
            .remove("sha_$players")
            .remove("file_$players")
            .remove("format_$players")
            .apply()
        synchronized(sessions) {
            if (sessions.values.none { it.file.absolutePath == oldFile.absolutePath }) oldFile.delete()
        }
        DiagnosticsStore.appendAssistant(context, "INFO", "assistant.model", "MODEL_DELETED",
            org.json.JSONObject().put("count", players))
    }

    fun import(context: Context, uri: Uri): ImportedPolicy {
        initialize(context)
        val importId = UUID.randomUUID().toString()
        val started = System.nanoTime()
        var stage = "metadata"
        var bytes = 0L
        var format = "unknown"
        var nativeStatus: Int? = null
        var rejectionReason = "import_failed"
        var parent: File? = null
        fun log(code: String, level: String = "INFO", error: Throwable? = null, players: Int? = null) {
            val fields = org.json.JSONObject().put("importId", importId).put("stage", stage)
                .put("format", format).put("bytes", bytes)
                .put("elapsedMs", TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - started))
            nativeStatus?.let { fields.put("nativeStatus", it) }
            players?.let { fields.put("count", it) }
            if (error != null) {
                fields.put("reason", rejectionReason).put("errorClass", error.javaClass.simpleName)
                // Keep useful ZIP/ORT errors while excluding provider URIs and local paths.
                val message = (error.message ?: "").replace(uri.toString(), "[source]")
                    .replace(Regex("(?:[A-Za-z]:[\\\\/]|/)[^\\s()]+"), "[path]")
                fields.put("errorMessage", message)
            }
            // Diagnostics must never turn a successful import into a failure.
            runCatching { DiagnosticsStore.appendAssistant(context, level, "assistant.model", code, fields) }
        }
        log("MODEL_IMPORT_STARTED")
        try {
            val displayName = context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
                ?.use { cursor -> if (cursor.moveToFirst()) cursor.getString(0) else null }
                ?: uri.takeIf { it.scheme == "file" }?.lastPathSegment ?: "policy.onnx"
            val lowerName = displayName.lowercase()
            val isPthExt = lowerName.endsWith(".pth") || lowerName.endsWith(".pt")
            val isOnnxExt = lowerName.endsWith(".onnx")
            stage = "extension"
            format = if (isPthExt) "pth" else if (isOnnxExt) "onnx" else "unknown"
            require(isPthExt || isOnnxExt) { "请选择 .onnx、.pth 或 .pt 模型文件" }

            val incoming = File(context.filesDir, "models/incoming/$importId").apply { mkdirs() }
            parent = incoming
            val staged = File(incoming, if (isPthExt) "policy.pth" else "policy.onnx")
            val digest = MessageDigest.getInstance("SHA-256")
            var treatAsPth = isPthExt
            stage = "read_source"
            val input = checkNotNull(context.contentResolver.openInputStream(uri)) { "无法读取所选文件" }
            input.buffered().use { source ->
                source.mark(4)
                val signature = IntArray(4) { source.read() }
                source.reset()
                if (isZipArchive(signature)) {
                    // PyTorch .pth checkpoints are ZIP archives containing data.pkl and data/* tensors.
                    treatAsPth = true
                }
                format = if (treatAsPth) "pth" else "onnx"
                log("MODEL_IMPORT_FORMAT_DETECTED")
                stage = "copy_source"
                staged.outputStream().buffered().use { output ->
                    val buffer = ByteArray(64 * 1024)
                    while (true) {
                        val count = source.read(buffer)
                        if (count < 0) break
                        bytes += count
                        require(bytes <= MAX_BYTES) { "模型文件超过 $MAX_MODEL_MIB MiB 限制" }
                        digest.update(buffer, 0, count)
                        output.write(buffer, 0, count)
                    }
                }
            }
            require(bytes > 0) { "模型文件为空" }
            val hash = digest.digest().joinToString("") { "%02x".format(it) }
            log("MODEL_IMPORT_FILE_COPIED")

            if (treatAsPth) {
                // The native ZIP reader validates archive limits before loading tensors.
                // Android's java.util.zip.ZipFile rejects some PyTorch ZIP64 archives.
                stage = "pth_archive_load_selftest"
                log("MODEL_IMPORT_VALIDATION_STARTED")
                val detectedPlayers = AiNative.loadMortalPth(0, staged.absolutePath, false)
                nativeStatus = detectedPlayers
                if (detectedPlayers == -7) {
                    rejectionReason = "invalid_pth_archive"
                    throw IllegalArgumentException("PyTorch 权重 ZIP 无效或超出限制（文件 300 MiB、解压 600 MiB、元数据 4 MiB、条目 5000）")
                }
                if (detectedPlayers != 4 && detectedPlayers != 3) {
                    rejectionReason = "invalid_mortal_pth_$detectedPlayers"
                    throw IllegalArgumentException("仅支持实验性四麻/三麻 Mortal v4；权重文件无效或结构不受支持 (状态码: $detectedPlayers)")
                }
                val players = detectedPlayers
                log("MODEL_IMPORT_VALIDATED", players = players)
                stage = "save_model"
                val folder = File(File(context.filesDir, "models"), "$players-$hash").apply { mkdirs() }
                val target = File(folder, "policy.pth")
                val temp = File(folder, "policy.tmp")
                staged.copyTo(temp, overwrite = true)
                if (target.exists()) temp.delete() else check(temp.renameTo(target)) { "无法保存模型文件" }
                val savedName = displayName.take(64)
                context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
                    .putString("name_$players", savedName)
                    .putString("sha_$players", hash)
                    .putString("file_$players", target.relativeTo(context.filesDir).path)
                    .putString("format_$players", "mortal-pth")
                    .putBoolean("enabled_$players", true)
                    .apply()
                stage = "complete"
                log("MODEL_IMPORTED", players = players)
                return ImportedPolicy(players, savedName, bytes, hash, "mortal-pth")
            }

            stage = "onnx_external_weights"
            log("MODEL_IMPORT_VALIDATION_STARTED")
            rejectExternalWeights(staged)
            stage = "onnx_load_selftest"
            val spec = validateOnnx(staged)
            val players = spec.players
            format = if (spec.contractKind == 1) "mortal-onnx" else "akagi-onnx"
            log("MODEL_IMPORT_VALIDATED", players = players)
            stage = "save_model"
            val folder = File(File(context.filesDir, "models"), "$players-$hash").apply { mkdirs() }
            val target = File(folder, "policy.onnx")
            val temp = File(folder, "policy.tmp")
            staged.copyTo(temp, overwrite = true)
            if (target.exists()) temp.delete() else check(temp.renameTo(target)) { "无法保存模型文件" }
            synchronized(sessions) {
                if (target.absolutePath !in sessions) {
                    sessions[target.absolutePath] = createValidatedCached(target, players)
                }
            }
            val formatTag = if (spec.contractKind == 1) "mortal-onnx" else "akagi-onnx"
            val savedName = spec.description.ifBlank { displayName }.take(64)
            context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
                .putString("name_$players", savedName)
                .putString("sha_$players", hash)
                .putString("file_$players", target.relativeTo(context.filesDir).path)
                .putString("format_$players", formatTag)
                .putBoolean("enabled_$players", true)
                .apply()
            stage = "complete"
            log("MODEL_IMPORTED", players = players)
            return ImportedPolicy(players, savedName, bytes, hash, formatTag)
        } catch (error: Throwable) {
            if (rejectionReason == "import_failed") rejectionReason = "${stage}_failed"
            log("MODEL_IMPORT_REJECTED", "ERROR", error)
            throw error
        } finally {
            parent?.let { incoming ->
                incoming.listFiles()?.forEach { it.delete() }
                incoming.delete()
            }
        }
    }

    private fun isZipArchive(signature: IntArray): Boolean =
        signature[0] == 0x50 && signature[1] == 0x4b &&
            ((signature[2] == 0x03 && signature[3] == 0x04) ||
                (signature[2] == 0x05 && signature[3] == 0x06) ||
                (signature[2] == 0x07 && signature[3] == 0x08))

    /** JNI calls this on the assistant worker thread for .onnx models (Akagi or Mortal). */
    @JvmStatic
    fun infer(players: Int, observation: FloatArray): FloatArray? {
        val context = appContext ?: return null
        if (players != 3 && players != 4) return null
        return try {
            synchronized(sessions) {
                if (players in failedPlayers) return null
                val cached = activeSessions[players] ?: return null
                run(cached.session, cached.spec, observation)
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
            val file = selectedFile(context, players)
            check(file.isFile) { "模型文件不存在" }
            val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            val format = prefs.getString("format_$players", "") ?: ""
            if (format == "mortal-pth" || file.name.endsWith(".pth", ignoreCase = true) || file.name.endsWith(".pt", ignoreCase = true)) {
                val rc = AiNative.loadMortalPth(players, file.absolutePath, true)
                check(rc == players) { "无法加载 Mortal .pth 模型 (状态码: $rc)" }
                synchronized(sessions) {
                    activeSessions.remove(players)
                    failedPlayers.remove(players)
                }
                return 0
            }
            synchronized(sessions) {
                val cached = sessions[file.absolutePath] ?: createValidatedCached(file, players)
                    .also { sessions[file.absolutePath] = it }
                AiNative.configureCustomContract(
                    players,
                    cached.spec.contractKind,
                    cached.spec.obsVersion,
                    cached.spec.obsChannels,
                    cached.spec.actionDim,
                )
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
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val format = prefs.getString("format_$players", "") ?: ""
        if (format == "mortal-pth" || file.name.endsWith(".pth", ignoreCase = true) || file.name.endsWith(".pt", ignoreCase = true)) {
            val rc = AiNative.loadMortalPth(players, file.absolutePath, false)
            check(rc == players) { "Mortal .pth 模型自检失败 (状态码: $rc)" }
            return "${players}人 Mortal (.pth) 加载与推理自检通过（实验性）· ${TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - started)} ms"
        }
        val cached = createValidatedCached(file, players)
        cached.session.use {
            run(it, cached.spec, FloatArray(cached.spec.obsChannels * cached.spec.tileDim), validation=true)
        }
        val kindLabel = if (cached.spec.contractKind == 1) "Mortal (.onnx)" else "Akagi (.onnx)"
        return "${players}人 $kindLabel 模型可用 · ${TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - started)} ms"
    }

    private fun validateOnnx(file: File, expectedPlayers: Int? = null): OnnxSpec {
        val session = createSession(file)
        session.use {
            return inspectAndVerifyOnnx(session, expectedPlayers)
        }
    }

    private fun createValidatedCached(file: File, players: Int): Cached {
        val session = createSession(file)
        try {
            val spec = inspectAndVerifyOnnx(session, players)
            return Cached(file, session, spec)
        } catch (error: Throwable) {
            session.close()
            throw error
        }
    }

    private fun inspectAndVerifyOnnx(session: OrtSession, expectedPlayers: Int?): OnnxSpec {
        val inputs = session.inputNames.associateWith { tensorInfo(session.inputInfo.getValue(it)) }
        val obs = inputs.entries.filter { it.value.shape.size == 3 }.singleOrNull()
            ?: throw IllegalArgumentException("需要唯一的 float32 观察输入 [1, C, T]")
        require(obs.value.type == OnnxJavaType.FLOAT)
        val shape = obs.value.shape
        require(shape[0] == 1L || shape[0] == -1L)
        val output = session.outputNames.map { it to tensorInfo(session.outputInfo.getValue(it)) }
            .filter { (_, t) -> t.type == OnnxJavaType.FLOAT && t.shape.size == 2 && t.shape[1].toInt() in KNOWN_ACTION_SPACES }
            .singleOrNull() ?: throw IllegalArgumentException("需要唯一的动作输出 [1, A]")
        require(output.second.shape[0] == 1L || output.second.shape[0] == -1L)
        val c = shape[1].toInt(); val t = shape[2].toInt(); val actions = output.second.shape[1].toInt()
        val meta = session.metadata
        fun value(key: String) = meta.getCustomMetadataValue(key).orElse("")
        val spec = when {
            (c == 39 && t == 34 && actions == 82) || (c == 37 && t == 27 && actions == 60) -> {
                val players = if (actions == 82) 4 else 3
                require(value("majmax.contract") == "akagi-policy-v1") { "Akagi 模型需要声明 akagi-policy-v1 协议" }
                require(value("majmax.players") == players.toString()) { "模型人数声明不匹配" }
                require(value("majmax.obs_schema") == "1") { "特征协议版本不匹配" }
                require(value("majmax.action_codec") == "riichienv-core-0.4.8") { "动作编号协议不匹配" }
                require(inputs.size == 1 && session.outputNames.size == 1) { "Akagi 模型不支持额外输入输出" }
                OnnxSpec(players,meta.description,0,1,c,t,actions,obs.key,output.first)
            }
            t == 34 && ((c == 1012 && actions == 46) || (c == 775 && actions == 44)) -> {
                val players = if (c == 1012) 4 else 3
                require(value("majmax.contract").let { it.isEmpty() || it.startsWith("mortal") }) { "模型协议与 Mortal v4 不匹配" }
                require(value("majmax.players").let { it.isEmpty() || it == players.toString() }) { "Mortal 模型人数声明不匹配" }
                val aux = inputs.filterKeys { it != obs.key }.mapValues { (_, info) ->
                    require(info.shape.size == 2 && (info.shape[0] == 1L || info.shape[0] == -1L) && info.shape[1] == actions.toLong()) { "Mortal 附加输入仅支持 [1,$actions] 合法动作 mask" }
                    require(info.type == OnnxJavaType.BOOL || info.type == OnnxJavaType.FLOAT) { "mask 仅支持 bool 或 float32" }
                    AuxInput(info.type,info.shape)
                }
                require(aux.size <= 1) { "Mortal 最多支持一个 mask 输入" }
                OnnxSpec(players,meta.description,1,4,c,t,actions,obs.key,output.first,aux)
            }
            else -> throw IllegalArgumentException("支持 Akagi 及实验性 Mortal v4：四麻 1012×34 → 46、三麻 775×34 → 44；旧版和其他结构暂不支持")
        }
        require(expectedPlayers == null || spec.players == expectedPlayers) { "模型适用人数不匹配" }
        run(session,spec,FloatArray(c*t),validation=true)
        return spec
    }

    private fun run(session: OrtSession, spec: OnnxSpec, observation: FloatArray, validation: Boolean = false): FloatArray {
        val size = spec.obsChannels * spec.tileDim
        require(observation.all(Float::isFinite)) { "观察特征无效" }
        val mask = if (spec.contractKind == 1) {
            if (validation) BooleanArray(spec.actionDim) { true } else {
                require(observation.size == size + spec.actionDim) { "缺少当前合法动作 mask" }
                BooleanArray(spec.actionDim) { i ->
                    val v = observation[size+i]; require(v == 0f || v == 1f); v == 1f
                }.also { require(it.any { legal -> legal }) }
            }
        } else { require(observation.size == size); BooleanArray(spec.actionDim) { true } }
        val resources = ArrayList<OnnxTensor>()
        val options = OrtSession.RunOptions()
        val timeout = timeoutScheduler.schedule({ runCatching { options.setTerminate(true) } },500,TimeUnit.MILLISECONDS)
        try {
            val feed = HashMap<String,OnnxTensor>()
            val obs = OnnxTensor.createTensor(environment,FloatBuffer.wrap(observation.copyOf(size)),longArrayOf(1,spec.obsChannels.toLong(),spec.tileDim.toLong()))
            resources += obs; feed[spec.obsInputName] = obs
            for ((name, aux) in spec.auxInputs) {
                val tensor = if (aux.type == OnnxJavaType.BOOL)
                    OnnxTensor.createTensor(environment,arrayOf(mask))
                else OnnxTensor.createTensor(environment,FloatBuffer.wrap(FloatArray(mask.size) { if(mask[it]) 1f else 0f }),longArrayOf(1,mask.size.toLong()))
                resources += tensor; feed[name] = tensor
            }
            session.run(feed,setOf(spec.actionOutputName),options).use { output ->
                val buffer = (output.get(spec.actionOutputName).orElseThrow() as OnnxTensor).floatBuffer
                val values = FloatArray(buffer.remaining()).also { buffer.get(it) }
                require(values.size == spec.actionDim) { "模型输出长度无效" }
                for (i in values.indices) {
                    if (mask[i]) require(values[i].isFinite() && (spec.contractKind == 0 || kotlin.math.abs(values[i]) <= 10000f)) { "模型合法动作输出无效" }
                    else { require(!values[i].isNaN()); values[i] = 0f } // Official Q graphs mask illegal slots to -infinity.
                }
                return values
            }
        } finally {
            resources.forEach { runCatching { it.close() } }
            timeout.cancel(false); options.close()
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
}
