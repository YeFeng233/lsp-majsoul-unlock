package com.yefeng.majmax.hookprobe.manager

import android.os.SystemClock
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import org.json.JSONObject
import java.util.UUID

data class AutoDiscardStatus(val enabled: Boolean = false, val message: String = "自动操作已暂停")
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
        if (enabled) { pause("已手动暂停自动操作"); return }
        val state = latest
        if (state == null || state.optString("status") != "live" || state.optBoolean("modelFallback")) {
            pause("请先连接并同步实时牌局，再开启自动操作"); return
        }
        enabled = true
        AutoDiscardState.mutable.value = AutoDiscardStatus(true, "AI 自动操作已开启")
        log("ENABLED")
        offer(state)
    }

    @Synchronized fun observe(result: JSONObject) {
        latest = JSONObject(result.toString())
        if (!enabled) return
        if (result.optString("status") != "live" || result.optBoolean("modelFallback")) {
            pause("同步或模型状态异常，自动操作已暂停"); return
        }
        result.optJSONObject("input")?.let { input ->
            val payload = input.optJSONObject("payload")
            val command = pending
            if (command == null || payload == null || !matchesInput(command, input.optString("method"), payload)) {
                pause("检测到手动或不匹配的操作，自动操作已暂停")
            } else {
                wireConfirmed = true
                log("UPLINK_CONFIRMED id=${command.optString("id")}")
                AutoDiscardState.mutable.value = AutoDiscardStatus(true, "已确认游戏发送操作 · 等待下一回合")
            }
            return
        }
        val command = pending
        if (command != null) {
            if (now() - pendingAt > 6_000) {
                pause("操作确认超时，已暂停；请手动检查")
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
        // No server prompt (for example after restoration): wait for the next synchronized action.
        if (result.optJSONArray("legalOperations")?.length() == 0) return
        val action = prepareAction(result, first) ?: run {
            pause("AI 动作与合法操作未能匹配，请手动检查")
            return
        }
        val key = "${result.optLong("sourceGeneration")}:${result.optString("connection")}:${result.optLong("revision")}" 
        if (key == attempted) return
        if (result.optLong("sourceSequence") <= 0) return
        attempted = key
        wireConfirmed = false; submitted = false; accepted = false
        pendingAt = now()
        pending = JSONObject(result.toString()).apply {
            remove("analysis"); remove("recommendations"); remove("input")
            put("id", "$serial-${++count}")
            put("action", action)
            put("gameTile", action.optString("tile")); put("tsumogiri", action.optBoolean("moqie"))
        }
        log("QUEUED id=${pending!!.optString("id")} kind=${action.optString("kind")}")
        AutoDiscardState.mutable.value = AutoDiscardStatus(true, "等待游戏操作窗口 · 可随时暂停")
    }

    @Synchronized fun poll(acknowledgements: String?): String? {
        if (acknowledgements != null) runCatching {
            val rows = org.json.JSONArray(acknowledgements)
            for (index in 0 until rows.length()) {
                val ack = rows.getJSONObject(index)
                if (ack.optString("state") == "manual") {
                    if (enabled) pause("检测到手动操作，自动操作已暂停")
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
                            "illegal_tile" -> "目标牌当前不可操作"
                            "illegal_combination" -> "鸣牌或立直组合已变化"
                            "target_mismatch" -> "目标牌或座位已变化"
                            "win_available" -> "可以和牌，请手动确认"
                            "special_selection" -> "正在选择特殊操作"
                            "failed" -> "服务器未确认操作"
                            else -> "游戏操作状态异常"
                        }
                        pause("$reason，自动操作已暂停")
                    }
                }
            }
        }.onFailure { pause("操作反馈格式异常，已暂停") }
        if (enabled && wireConfirmed && accepted && latest?.optLong("revision") != pending?.optLong("revision")) {
            pending = null
            latest?.let(::offer)
        }
        if (pending != null && now() - pendingAt > 6_000) pause("操作确认超时，已暂停；请手动检查")
        return if (enabled && !submitted && !wireConfirmed) pending?.toString() else null
    }

    companion object {
        /** Select an exact server combination, including red fives; no fallback action. */
        fun prepareAction(result: JSONObject, first: JSONObject): JSONObject? {
            val kind = first.optString("kind")
            val context = result.optJSONObject("autoContext") ?: return null
            val own = result.optInt("seat", -1)
            val target = first.optInt("target", -1)
            val response = context.optString("phase") == "wait_response"
            val type = when (kind) {
                "discard" -> 1; "chi" -> 2; "pon" -> 3; "ankan" -> 4; "kan" -> 5
                "kakan" -> 6; "riichi" -> 7; "hora" -> if (target == own) 8 else 9
                "abort" -> 10; "kita" -> 11; "pass" -> 0; else -> return null
            }
            val claim = type in listOf(2,3,5,9) || (kind == "pass" && response)
            if (kind != "discard" && kind != "pass" && claim != response) return null
            if (kind == "hora" || kind in listOf("chi","pon","kan")) {
                if (target !in 0 until result.optInt("players", 4) || (claim && target == own)) return null
            }
            val out = JSONObject(first.toString()).put("type", type)
                .put("method", if (claim) "inputChiPengGang" else "inputOperation")
            val operations = result.optJSONArray("legalOperations") ?: org.json.JSONArray()
            val op = (0 until operations.length()).map { operations.getJSONObject(it) }.firstOrNull { it.optInt("type") == type }
            val combo = op?.optJSONArray("combination") ?: org.json.JSONArray()
            fun gameTiles(values: org.json.JSONArray?): List<String>? {
                if (values == null) return null
                return (0 until values.length()).map { toGameTile(values.optString(it)) ?: return null }
            }
            fun split(value: String): List<String>? {
                val tiles = value.split('|')
                return tiles.takeIf { it.isNotEmpty() && it.all { t -> t.matches(Regex("[0-9][mps]|[1-7]z")) } }
            }
            val hand = gameTiles(result.optJSONArray("hand")) ?: return null
            if (kind == "pass") {
                // Own-turn optional actions must be resolved to a discard by the engine.
                if (!response || operations.length() == 0) return null
                return out.put("cancel_operation", true)
            }
            if (op == null) return null
            if (kind == "discard" || kind == "riichi") {
                val tile = toGameTile(first.optString("tile")) ?: return null
                if (tile !in hand || (kind == "discard" && !first.has("tsumogiri"))) return null
                if (kind == "riichi" && (context.optBoolean("riichi") || !(0 until combo.length()).any { combo.optString(it).replace('0','5') == tile.replace('0','5') })) return null
                val moqie = if (kind == "riichi") first.optString("tile") == context.optString("drawnTile") else first.getBoolean("tsumogiri")
                if (context.optBoolean("riichi") && !moqie) return null
                return out.put("tile", tile).put("moqie", moqie)
            }
            if (kind == "kita") {
                if (result.optInt("players") != 3 || "4z" !in hand) return null
                return out.put("moqie", context.optString("drawnTile") == "N")
            }
            if (kind in listOf("chi","pon","kan","ankan","kakan")) {
                val consumed = gameTiles(first.optJSONArray("consumed")) ?: return null
                val need = when(kind) { "chi","pon" -> 2; "kan" -> 3; "ankan" -> 4; else -> 3 }
                if (consumed.size != need) return null
                val remaining = hand.toMutableList()
                if (kind != "kakan" && !consumed.all { remaining.remove(it) }) return null
                var wanted = consumed
                if (kind == "kakan") {
                    val added = toGameTile(first.optString("tile")) ?: return null
                    if (added !in hand) return null
                    val melds = context.optJSONArray("melds") ?: return null
                    fun base(t: String) = t.replace('0','5')
                    val pon = (0 until melds.length()).map { melds.getJSONObject(it) }.firstOrNull {
                        it.optString("kind") == "pon" && gameTiles(it.optJSONArray("tiles"))?.all { t -> base(t) == base(added) } == true
                    } ?: return null
                    val tiles = gameTiles(pon.optJSONArray("tiles")) ?: return null
                    if (tiles.size != 3 || tiles.sorted() != consumed.sorted()) return null
                    wanted = tiles + added
                }
                val index = (0 until combo.length()).firstOrNull { split(combo.optString(it))?.sorted() == wanted.sorted() } ?: return null
                out.put("index", index).put("combination", combo.getString(index))
                if (claim) out.put("tile", toGameTile(first.optString("tile")) ?: return null)
                return out
            }
            return out.put("index", 0)
        }

        fun matchesInput(command: JSONObject, method: String, payload: JSONObject): Boolean {
            val action = command.optJSONObject("action") ?: return false
            if (method != ".lq.FastTest.${action.optString("method")}") return false
            if (action.optBoolean("cancel_operation")) return payload.optBoolean("cancel_operation") && payload.optInt("type") == 0
            if (payload.optBoolean("cancel_operation") || payload.optInt("type") != action.optInt("type")) return false
            for (key in listOf("tile","moqie","index")) {
                if (key == "tile" && action.optString("kind") !in listOf("discard","riichi")) continue // Other RPCs identify the operation by type/index.
                if (action.has(key)) {
                    if (key == "moqie" && payload.optBoolean(key) != action.optBoolean(key)) return false
                    if (key == "index" && payload.optInt(key) != action.optInt(key)) return false
                    if (key == "tile" && payload.optString(key) != action.optString(key)) return false
                }
            }
            return true
        }

        fun toGameTile(tile: String): String? {
            val honors = mapOf("E" to "1z", "S" to "2z", "W" to "3z", "N" to "4z", "P" to "5z", "F" to "6z", "C" to "7z")
            honors[tile]?.let { return it }
            if (!tile.matches(Regex("[1-9][mps]|5[mps]r"))) return null
            return if (tile.endsWith("r")) "0${tile[1]}" else tile
        }
    }
}
