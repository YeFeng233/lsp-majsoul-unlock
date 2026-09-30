package com.yefeng.majmax.hookprobe.manager

import android.os.SystemClock
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.json.JSONObject
import java.util.UUID

data class AutoDiscardStatus(val enabled: Boolean = false, val message: String = "自动切牌已暂停")
object AutoDiscardState {
    internal val mutable = MutableStateFlow(AutoDiscardStatus())
    val state = mutable.asStateFlow()
}

/** One pending command, one attempt per decision window; never retries an input. */
internal class AutoDiscardController(private val log: (String) -> Unit,
    private val now: () -> Long = SystemClock::elapsedRealtime) {
    private var enabled = false
    private var latest: JSONObject? = null
    private var pending: JSONObject? = null
    private var attempted = ""
    private var pendingAt = 0L
    private var serial = UUID.randomUUID().toString()
    private var count = 0L
    private var wireConfirmed = false
    private var submitted = false
    private var accepted = false

    @Synchronized fun pause(reason: String) {
        if (enabled || pending != null) log("PAUSED: $reason")
        enabled = false; pending = null
        AutoDiscardState.mutable.value = AutoDiscardStatus(false, reason)
    }

    @Synchronized fun invalidate(reason: String) {
        latest = null
        pause(reason)
    }

    @Synchronized fun toggle() {
        if (enabled) { pause("已手动暂停自动切牌"); return }
        val state = latest
        if (state == null || state.optString("status") != "live" || state.optBoolean("modelFallback")) {
            pause("请先连接并同步实时牌局，再开启自动切牌"); return
        }
        enabled = true
        AutoDiscardState.mutable.value = AutoDiscardStatus(true, "自动切牌已开启 · 其他操作需手动")
        log("ENABLED")
        offer(state)
    }

    @Synchronized fun observe(result: JSONObject) {
        latest = JSONObject(result.toString())
        if (!enabled) return
        if (result.optString("status") != "live" || result.optBoolean("modelFallback")) {
            pause("同步或模型状态异常，自动切牌已暂停"); return
        }
        result.optJSONObject("input")?.let { input ->
            val payload = input.optJSONObject("payload")
            val command = pending
            if (command == null || input.optString("method") != ".lq.FastTest.inputOperation" ||
                payload?.optInt("type") != 1 || payload.optString("tile") != command.optString("gameTile") ||
                payload.optBoolean("moqie") != command.optBoolean("tsumogiri")) {
                pause("检测到手动或其他出牌，自动切牌已暂停")
            } else {
                wireConfirmed = true
                log("UPLINK_CONFIRMED id=${command.optString("id")}")
                AutoDiscardState.mutable.value = AutoDiscardStatus(true, "已确认游戏发送切牌 · 等待下一回合")
            }
            return
        }
        val command = pending
        if (command != null) {
            if (now() - pendingAt > 6_000) {
                pause("出牌确认超时，已暂停；请手动检查")
            } else if (wireConfirmed && accepted && result.optLong("revision") != command.optLong("revision")) {
                pending = null
                offer(result)
            }
            return
        }
        offer(result)
    }

    private fun offer(result: JSONObject) {
        if (!enabled || pending != null || !result.optBoolean("canAct") || result.optBoolean("inputPending")) return
        val first = result.optJSONArray("recommendations")?.optJSONObject(0) ?: return
        if (first.optString("kind") != "discard") {
            AutoDiscardState.mutable.value = AutoDiscardStatus(true, "等待手动操作：立直、鸣牌、和牌及跳过")
            return
        }
        val context = result.optJSONObject("autoContext") ?: return
        if (context.optBoolean("riichi")) {
            AutoDiscardState.mutable.value = AutoDiscardStatus(true, "已立直，交由游戏或手动操作")
            return
        }
        val key = "${result.optLong("sourceGeneration")}:${result.optString("connection")}:${result.optLong("revision")}" 
        if (key == attempted) return
        val gameTile = toGameTile(first.optString("tile")) ?: return
        if (!first.has("tsumogiri") || result.optLong("sourceSequence") <= 0) return
        attempted = key
        wireConfirmed = false; submitted = false; accepted = false
        pendingAt = now()
        pending = JSONObject(result.toString()).apply {
            remove("analysis"); remove("recommendations"); remove("input")
            put("id", "$serial-${++count}")
            put("gameTile", gameTile); put("tsumogiri", first.getBoolean("tsumogiri"))
        }
        log("QUEUED id=${pending!!.optString("id")} tile=$gameTile")
        AutoDiscardState.mutable.value = AutoDiscardStatus(true, "等待游戏操作窗口 · 可随时暂停")
    }

    @Synchronized fun poll(acknowledgements: String?): String? {
        if (acknowledgements != null) runCatching {
            val rows = org.json.JSONArray(acknowledgements)
            for (index in 0 until rows.length()) {
                val ack = rows.getJSONObject(index)
                if (ack.optString("state") == "manual") {
                    if (enabled) pause("检测到手动操作，自动切牌已暂停")
                    continue
                }
                if (ack.optString("id") != pending?.optString("id")) continue
                when (ack.optString("state")) {
                    "submitted" -> { submitted = true; log("GAME_SUBMITTED id=${ack.optString("id")}") }
                    "accepted" -> { accepted = true; log("SERVER_ACCEPTED id=${ack.optString("id")}") }
                    else -> {
                        log("GAME_REJECTED ${ack.optString("state")} ${ack.optString("reason")}")
                        val reason = when (ack.optString("state")) {
                            "stale" -> "推荐已过期"
                            "round_mismatch" -> "回合校验未通过"
                            "hand_mismatch", "invalid_hand", "unknown_hand" -> "手牌校验未通过"
                            "illegal_tile" -> "目标牌当前不可切"
                            "win_available" -> "可以和牌，请手动确认"
                            "special_selection" -> "正在选择特殊操作"
                            "failed" -> "服务器未确认切牌"
                            else -> "游戏操作状态异常"
                        }
                        pause("$reason，自动切牌已暂停")
                    }
                }
            }
        }.onFailure { pause("出牌反馈格式异常，已暂停") }
        if (enabled && wireConfirmed && accepted && latest?.optLong("revision") != pending?.optLong("revision")) {
            pending = null
            latest?.let(::offer)
        }
        if (pending != null && now() - pendingAt > 6_000) pause("出牌确认超时，已暂停；请手动检查")
        return if (enabled && !submitted && !wireConfirmed) pending?.toString() else null
    }

    companion object {
        fun toGameTile(tile: String): String? {
            val honors = mapOf("E" to "1z", "S" to "2z", "W" to "3z", "N" to "4z", "P" to "5z", "F" to "6z", "C" to "7z")
            honors[tile]?.let { return it }
            if (!tile.matches(Regex("[1-9][mps]|5[mps]r"))) return null
            return if (tile.endsWith("r")) "0${tile[1]}" else tile
        }
    }
}
