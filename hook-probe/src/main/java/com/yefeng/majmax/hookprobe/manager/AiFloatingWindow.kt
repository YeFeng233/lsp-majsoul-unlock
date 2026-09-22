package com.yefeng.majmax.hookprobe.manager

import android.content.Context
import android.graphics.Color
import android.graphics.PixelFormat
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import android.view.WindowManager
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.SeekBar
import android.widget.TextView
import org.json.JSONArray
import org.json.JSONObject
import java.util.Locale
import kotlin.math.abs
import kotlin.math.roundToInt

/** A small native overlay: only its bounds receive touches; text stays opaque. */
internal class AiFloatingWindow(private val context: Context, private val stop: () -> Unit) {
    private val windows = context.getSystemService(WindowManager::class.java)
    private val preferences = context.getSharedPreferences("ai-window", Context.MODE_PRIVATE)
    private val green = Color.rgb(114, 236, 176)
    private val muted = Color.rgb(200, 213, 206)
    private var opacity = preferences.getInt("opacity", 78).coerceIn(25, 95)
    private var width = preferences.getInt("width", 300).coerceIn(220, 420)
    private var collapsed = preferences.getBoolean("collapsed", false)
    private var settings = false
    private var result = JSONObject().put("status", "waiting").put("message", "等待牌局连接")
    private var root: LinearLayout? = null
    private var body: LinearLayout? = null
    private var title: TextView? = null
    private val layout = WindowManager.LayoutParams(
        dp(width), WindowManager.LayoutParams.WRAP_CONTENT,
        WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
        WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE or WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL,
        PixelFormat.TRANSLUCENT,
    ).apply {
        gravity = Gravity.TOP or Gravity.START
        x = preferences.getInt("x", dp(16))
        y = preferences.getInt("y", dp(76))
    }

    fun show() { rebuild() }
    fun update(value: JSONObject) {
        result = value
        title?.text = if (collapsed) "AI" else if (value.optString("status") == "demo") "本地自检 · 样例" else "雀魂 · 本地 AI"
        renderBody()
    }
    fun close() {
        root?.let { runCatching { windows.removeView(it) } }
        root = null
    }
    fun configurationChanged() { rebuild() }

    private fun dp(value: Int) = (value * context.resources.displayMetrics.density).roundToInt()
    private fun background() = GradientDrawable().apply {
        setColor(Color.argb((255 * opacity / 100f).roundToInt(), 15, 30, 25))
        cornerRadius = dp(if (collapsed) 24 else 18).toFloat()
        setStroke(dp(1), Color.argb(110, 139, 201, 164))
    }
    private fun label(value: String, size: Float = 13f, color: Int = Color.WHITE, bold: Boolean = false) =
        TextView(context).apply {
            text = value
            textSize = size
            setTextColor(color)
            if (bold) setTypeface(typeface, Typeface.BOLD)
            setPadding(dp(4), dp(4), dp(4), dp(4))
        }
    private fun action(value: String, description: String, click: () -> Unit) =
        label(value, 13f, green, true).apply {
            gravity = Gravity.CENTER
            contentDescription = description
            layoutParams = LinearLayout.LayoutParams(dp(44), dp(44))
            setOnClickListener { click() }
        }

    private fun rebuild() {
        close()
        val panel = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            background = background()
            elevation = dp(4).toFloat()
        }
        root = panel
        val heading = LinearLayout(context).apply { gravity = Gravity.CENTER_VERTICAL }
        val caption = label(if (collapsed) "AI" else if (result.optString("status") == "demo") "本地自检 · 样例" else "雀魂 · 本地 AI",
            if (collapsed) 16f else 13f, green, true)
        title = caption
        caption.gravity = if (collapsed) Gravity.CENTER else Gravity.CENTER_VERTICAL
        caption.setPadding(dp(if (collapsed) 0 else 12), 0, 0, 0)
        caption.contentDescription = if (collapsed) "展开 AI 悬浮窗，可拖动" else "拖动悬浮窗"
        heading.addView(caption, LinearLayout.LayoutParams(if (collapsed) dp(48) else 0, dp(48), if (collapsed) 0f else 1f))
        makeDraggable(caption) { if (collapsed) { collapsed = false; save(); rebuild() } }
        if (!collapsed) {
            heading.addView(action("调节", "调节透明度和宽度") { settings = !settings; rebuild() })
            heading.addView(action("收纳", "收纳成浮标") { collapsed = true; save(); rebuild() })
            heading.addView(action("×", "关闭 AI 助手", stop))
        }
        panel.addView(heading)
        body = null
        if (!collapsed) {
            val scroll = object : ScrollView(context) {
                override fun onMeasure(widthMeasureSpec: Int, heightMeasureSpec: Int) {
                    val max = (context.resources.displayMetrics.heightPixels - dp(128)).coerceAtLeast(dp(140))
                    super.onMeasure(widthMeasureSpec, View.MeasureSpec.makeMeasureSpec(max, View.MeasureSpec.AT_MOST))
                }
            }
            val content = LinearLayout(context).apply {
                orientation = LinearLayout.VERTICAL
                setPadding(dp(10), 0, dp(10), dp(10))
            }
            body = content
            scroll.addView(content)
            panel.addView(scroll)
            renderBody()
        }
        layout.width = if (collapsed) dp(48) else dp(width).coerceAtMost(context.resources.displayMetrics.widthPixels - dp(12))
        clampPosition()
        windows.addView(panel, layout)
        panel.post { clampPosition(); runCatching { windows.updateViewLayout(panel, layout) } }
    }

    private fun renderBody() {
        val content = body ?: return
        content.removeAllViews()
        if (settings) {
            addSlider(content, "背景透明度", 25, 95, opacity, { "不透明度 $it%" }) {
                opacity = it; root?.background = background(); save()
            }
            addSlider(content, "窗口宽度", 220, 420, width, { "$it dp" }) {
                width = it
                layout.width = dp(width).coerceAtMost(context.resources.displayMetrics.widthPixels - dp(12))
                clampPosition()
                root?.let { panel -> windows.updateViewLayout(panel, layout) }
                save()
            }
            content.addView(label("拖动标题栏移动；点“收纳”变成小浮标。", 11f, muted))
            return
        }
        val status = result.optString("status")
        content.addView(label(result.optString("message", "等待牌局"), 12f,
            if (status == "demo") Color.rgb(255, 211, 131) else muted))
        if (status != "live" && status != "demo") {
            content.addView(label("先开启助手，再启动雀魂进入对局。中途开启或重连后，需等待完整牌局同步。", 12f, muted))
            return
        }
        content.addView(label(result.optString("round").replace("E", "东").replace("S", "南")
            .replace("W", "西").replace("N", "北") + " · ${result.optInt("players", 4)}人", 12f, muted))
        val analysis = result.optJSONObject("analysis") ?: return
        val rows = result.optJSONArray("recommendations") ?: JSONArray()
        if (rows.length() == 0) {
            content.addView(label("等待操作机会", 21f, green, true))
            analysis.optJSONObject("hand13")?.let { content.addView(label(handDescription(it), 12f, muted)) }
        }
        for (i in 0 until minOf(3, rows.length())) {
            val row = rows.getJSONObject(i)
            val tile = row.optString("tile")
            val kind = row.optString("kind")
            val name = when (kind) {
                "discard" -> "切 ${tileName(tile)}"
                "riichi" -> if (tile.isBlank()) "立直" else "立直 · 切 ${tileName(tile)}"
                "pon" -> "碰 ${tileName(tile)}"
                "chi" -> "吃 ${tileName(tile)}"
                "kan", "ankan", "kakan" -> "杠 ${tileName(tile)}"
                "hora" -> "和牌"
                "kita" -> "拔北"
                "abort" -> "九种九牌流局"
                else -> "跳过"
            }
            content.addView(label((if (i == 0) "推荐  " else "备选  ") + name,
                if (i == 0) 23f else 16f, if (i == 0) green else Color.WHITE, true))
            if (kind == "discard" || kind == "riichi") {
                discardResult(analysis, tile)?.let { hand -> content.addView(label(handDescription(hand), 12f, muted)) }
                val index = tileIndex(tile)
                val risks = analysis.optJSONArray("mixed_risk")
                if (index >= 0 && risks != null && index < risks.length()) {
                    content.addView(label("放铳风险指数  ${number(risks.optDouble(index))}", 12f, muted))
                }
            }
        }
        val defence = analysis.optString("best_defence_discard").takeUnless { it == "null" || it.isBlank() }
        if (defence != null && rows.length() > 0) content.addView(label("防守参考  ${tileName(defence)}", 12f, muted))
        val opponents = analysis.optJSONArray("opponents") ?: JSONArray()
        val riichiSeats = (0 until opponents.length()).map { opponents.getJSONObject(it) }
            .filter { it.optBoolean("is_riichi") }.map { relativeSeat(it.optInt("seat"), result.optInt("seat"), result.optInt("players", 4)) }
        if (riichiSeats.isNotEmpty()) content.addView(label("已立直：${riichiSeats.joinToString("、")}", 12f, Color.rgb(255, 191, 120)))
        content.addView(label("和牌率为听牌后的估计；风险指数越低越好，含打点权重，并非实际放铳概率。", 10f, muted))
        val duration = if (status == "demo") result.optLong("loadAndInferenceMs") else result.optLong("elapsedMs")
        content.addView(label("手机本地计算 · ${duration} ms${if (status == "demo") "（含模型加载）" else ""}", 10f, muted))
    }

    private fun addSlider(parent: LinearLayout, name: String, min: Int, max: Int, value: Int,
        format: (Int) -> String, change: (Int) -> Unit) {
        val caption = label("$name · ${format(value)}", 12f, muted)
        parent.addView(caption)
        parent.addView(SeekBar(context).apply {
            this.max = max - min
            progress = value - min
            contentDescription = name
            setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
                override fun onProgressChanged(seekBar: SeekBar?, progress: Int, fromUser: Boolean) {
                    if (fromUser) { caption.text = "$name · ${format(progress + min)}"; change(progress + min) }
                }
                override fun onStartTrackingTouch(seekBar: SeekBar?) {}
                override fun onStopTrackingTouch(seekBar: SeekBar?) {}
            })
        }, LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, dp(42)))
    }

    private fun makeDraggable(view: View, click: () -> Unit) {
        var downX = 0f; var downY = 0f; var originX = 0; var originY = 0; var moved = false
        val slop = ViewConfiguration.get(context).scaledTouchSlop
        view.setOnClickListener { click() }
        view.setOnTouchListener { target, event ->
            when (event.actionMasked) {
                MotionEvent.ACTION_DOWN -> {
                    downX = event.rawX; downY = event.rawY; originX = layout.x; originY = layout.y; moved = false
                    true
                }
                MotionEvent.ACTION_MOVE -> {
                    if (abs(event.rawX - downX) > slop || abs(event.rawY - downY) > slop) moved = true
                    if (moved) {
                        layout.x = originX + (event.rawX - downX).roundToInt()
                        layout.y = originY + (event.rawY - downY).roundToInt()
                        clampPosition()
                        root?.let { windows.updateViewLayout(it, layout) }
                    }
                    true
                }
                MotionEvent.ACTION_UP -> { save(); if (!moved) target.performClick(); true }
                MotionEvent.ACTION_CANCEL -> { save(); true }
                else -> false
            }
        }
    }
    private fun clampPosition() {
        val display = context.resources.displayMetrics
        layout.x = layout.x.coerceIn(0, (display.widthPixels - layout.width).coerceAtLeast(0))
        layout.y = layout.y.coerceIn(0, (display.heightPixels - (root?.height?.takeIf { it > 0 } ?: dp(48)) - dp(32)).coerceAtLeast(0))
    }
    private fun save() {
        preferences.edit().putInt("x", layout.x).putInt("y", layout.y).putInt("opacity", opacity)
            .putInt("width", width).putBoolean("collapsed", collapsed).apply()
    }

    private fun discardResult(analysis: JSONObject, tile: String): JSONObject? {
        val hand = analysis.optJSONObject("hand14") ?: return null
        for (group in listOf("maintain", "backwards")) {
            val choices = hand.optJSONArray(group) ?: continue
            for (i in 0 until choices.length()) {
                val candidate = choices.getJSONObject(i)
                if (candidate.optString("discard").removeSuffix("r") == tile.removeSuffix("r")) return candidate.optJSONObject("result")
            }
        }
        return null
    }
    private fun handDescription(hand: JSONObject): String {
        val shanten = hand.optInt("shanten", 99)
        val stage = when { shanten == 0 -> "听牌"; shanten < 0 -> "和牌形"; else -> "${shanten}向听" }
        val chance = if (shanten == 0) "估计和牌率 ${number(hand.optDouble("avg_agari_rate"))}%" else "和牌率 —（未听牌）"
        return "$stage · 进张 ${hand.optInt("waits_total")} 枚\n$chance${if (hand.optBoolean("is_furiten")) " · 振听" else ""}"
    }
    private fun number(value: Double) = if (value.isFinite()) String.format(Locale.ROOT, "%.1f", value) else "—"
    private fun relativeSeat(seat: Int, own: Int, players: Int): String = when ((seat - own + players) % players) {
        1 -> "下家"; 2 -> if (players == 3) "上家" else "对家"; else -> "上家"
    }
    private fun tileName(tile: String): String {
        val honors = mapOf("E" to "东", "S" to "南", "W" to "西", "N" to "北", "P" to "白", "F" to "发", "C" to "中")
        honors[tile]?.let { return it }
        if (tile.length < 2) return "—"
        val suit = when (tile[1]) { 'm' -> "万"; 'p' -> "筒"; 's' -> "索"; else -> "" }
        return (if (tile.endsWith("r")) "赤" else "") + tile[0] + suit
    }
    private fun tileIndex(tile: String): Int {
        val honor = listOf("E", "S", "W", "N", "P", "F", "C").indexOf(tile)
        if (honor >= 0) return 27 + honor
        if (tile.length < 2) return -1
        val rank = tile[0].digitToIntOrNull()?.takeIf { it in 1..9 } ?: return -1
        return when (tile[1]) { 'm' -> rank - 1; 'p' -> rank + 8; 's' -> rank + 17; else -> -1 }
    }
}
