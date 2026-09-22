package com.yefeng.majmax.hookprobe.manager

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.res.Configuration
import android.net.LocalServerSocket
import android.net.LocalSocket
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.provider.Settings
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.json.JSONObject
import java.io.DataInputStream
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

data class AiServiceStatus(val running: Boolean = false, val connected: Boolean = false,
    val message: String = "助手未开启")

object AiStatus {
    internal val mutable = MutableStateFlow(AiServiceStatus())
    val state = mutable.asStateFlow()
}

class AiOverlayService : Service() {
    companion object {
        const val GAME = "com.soulgamechst.majsoul"
        private const val LIVE = "local-ai.LIVE"
        private const val DEMO = "local-ai.DEMO"
        private const val STOP = "local-ai.STOP"
        fun start(context: Context, demo: Boolean = false) {
            ContextCompat.startForegroundService(context,
                Intent(context, AiOverlayService::class.java).setAction(if (demo) DEMO else LIVE))
        }
        fun stop(context: Context) { context.stopService(Intent(context, AiOverlayService::class.java)) }
    }

    private data class Packet(val epoch: Long, val revision: Long, val connection: Long = 0,
        val kind: Int, val bytes: ByteArray = byteArrayOf(), val message: String = "")
    private val alive = AtomicBoolean(true)
    private val demoMode = AtomicBoolean(false)
    private val epoch = AtomicLong(0)
    private val revision = AtomicLong(0)
    private val queue = ArrayBlockingQueue<Packet>(64)
    private val main = Handler(Looper.getMainLooper())
    @Volatile private var server: LocalServerSocket? = null
    @Volatile private var client: LocalSocket? = null
    @Volatile private var connected = false
    private var reader: Thread? = null
    private var worker: Thread? = null
    private var overlay: AiFloatingWindow? = null

    override fun onCreate() {
        super.onCreate()
        val notifications = getSystemService(NotificationManager::class.java)
        notifications.createNotificationChannel(NotificationChannel("local-ai", "本地牌局助手",
            NotificationManager.IMPORTANCE_LOW))
        val open = PendingIntent.getActivity(this, 0, Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        val stop = PendingIntent.getService(this, 1, Intent(this, AiOverlayService::class.java).setAction(STOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
        startForeground(105, NotificationCompat.Builder(this, "local-ai")
            .setSmallIcon(android.R.drawable.ic_dialog_info).setContentTitle("雀魂本地助手已开启")
            .setContentText("手机本地分析 · 点击返回管理 · 可随时停止")
            .setContentIntent(open).setOngoing(true).addAction(0, "停止助手", stop).build())
        if (!Settings.canDrawOverlays(this)) {
            AiStatus.mutable.value = AiServiceStatus(message = "请先允许显示在其他应用上层")
            stopSelf()
            return
        }
        overlay = AiFloatingWindow(this) { stopSelf() }.also { it.show() }
        worker = Thread({ consume() }, "majmax-ai-engine").also { it.start() }
        reader = Thread({ listen() }, "majmax-ai-reader").also { it.start() }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == STOP) { stopSelf(); return START_NOT_STICKY }
        demoMode.set(intent?.action == DEMO)
        // A mode change has its own generation. Previously queued results may
        // never overwrite the self-test label or a new live session.
        reset(if (demoMode.get()) "正在加载本地模型自检…" else "等待游戏连接，请先开启助手再进入牌局")
        if (demoMode.get()) {
            queue.offer(Packet(epoch.get(), revision.incrementAndGet(), kind = 5))
        }
        runCatching { client?.close() }
        return START_NOT_STICKY
    }

    private fun reset(message: String) {
        val generation = epoch.incrementAndGet()
        queue.clear()
        val ticket = revision.incrementAndGet()
        queue.offer(Packet(generation, ticket, kind = 3, message = message))
        post(JSONObject().put("status", "waiting").put("message", message), generation, ticket)
    }

    private fun listen() {
        try {
            val expectedUid = packageManager.getApplicationInfo(GAME, 0).uid
            val listener = LocalServerSocket("majmax.local-ai.v1")
            server = listener
            while (alive.get()) {
                val socket = listener.accept()
                if (socket.peerCredentials.uid != expectedUid) { socket.close(); continue }
                client = socket
                connected = true
                if (!demoMode.get()) reset("Hook 已连接，等待进入牌局；中途开启请重新进入")
                try {
                    socket.soTimeout = 8_000
                    val input = DataInputStream(socket.inputStream)
                    var sourceGeneration: Long? = null
                    while (alive.get()) {
                        val size = input.readInt()
                        require(size in 0..1_048_576)
                        val source = input.readLong()
                        val connection = input.readLong()
                        val kind = input.readUnsignedByte()
                        require(kind in listOf(0, 1, 2, 4))
                        val data = ByteArray(size)
                        input.readFully(data)
                        if (demoMode.get()) continue
                        if (sourceGeneration != null && sourceGeneration != source) {
                            reset("检测到消息丢失，请重新进入牌局同步")
                        }
                        sourceGeneration = source
                        if (kind == 4) continue
                        val packet = Packet(epoch.get(), revision.incrementAndGet(), connection, kind, data)
                        if (!queue.offer(packet)) {
                            reset("分析未跟上牌局，请重新进入牌局同步")
                            break
                        }
                    }
                } catch (_: Exception) {
                    // No raw frames, credentials or account identifiers are logged.
                } finally {
                    runCatching { socket.close() }
                    client = null
                    connected = false
                    if (alive.get() && !demoMode.get()) reset("游戏连接已断开，等待重新同步")
                }
            }
        } catch (_: Exception) {
            if (alive.get()) reset("采集服务未启动，请确认游戏已安装并重新开启助手")
        }
    }

    private fun consume() {
        var last = JSONObject().put("status", "waiting").put("message", "等待牌局")
        try {
            AiNative.reset()
            while (alive.get()) {
                val packet = queue.take()
                if (packet.epoch != epoch.get()) continue
                val result = when (packet.kind) {
                    3 -> { AiNative.reset(); JSONObject().put("status", "waiting").put("message", packet.message).toString() }
                    5 -> AiNative.selfTest()
                    else -> AiNative.frame(packet.connection, packet.kind, packet.bytes)
                }
                if (packet.epoch != epoch.get()) continue
                if (result != null) last = JSONObject(result)
                // Process every event, but present only the latest state after
                // a burst; stale inference is never posted over newer input.
                if (queue.isEmpty()) post(last, packet.epoch, packet.revision)
            }
        } catch (_: InterruptedException) {
            // Normal service shutdown.
        } catch (_: Throwable) {
            val failed = JSONObject().put("status", "error").put("message", "本地引擎加载失败，请重新开启助手")
            post(failed, epoch.get(), revision.get())
        }
    }

    private fun post(result: JSONObject, generation: Long, ticket: Long) {
        main.post {
            if (!alive.get() || epoch.get() != generation || revision.get() != ticket) return@post
            overlay?.update(result)
            AiStatus.mutable.value = AiServiceStatus(true, connected, result.optString("message"))
        }
    }

    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        overlay?.configurationChanged()
    }

    override fun onDestroy() {
        alive.set(false)
        epoch.incrementAndGet()
        runCatching { client?.close() }
        runCatching { server?.close() }
        reader?.interrupt()
        worker?.interrupt()
        queue.clear()
        main.removeCallbacksAndMessages(null)
        overlay?.close()
        AiStatus.mutable.value = AiServiceStatus()
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null
}
