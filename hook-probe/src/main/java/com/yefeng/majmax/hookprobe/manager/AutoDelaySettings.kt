package com.yefeng.majmax.hookprobe.manager

import android.content.Context
import java.util.Locale
import java.util.concurrent.ThreadLocalRandom

/** Store tenths of a second so slider steps survive saving without float rounding. */
internal data class AutoDelayRange(val minTenths: Int = 20, val maxTenths: Int = 50) {
    init { require(minTenths in 0..50 && maxTenths in minTenths..50) }

    fun withMinimum(value: Int) = value.coerceIn(0, 50).let { copy(minTenths = it, maxTenths = maxOf(it, maxTenths)) }
    fun withMaximum(value: Int) = value.coerceIn(0, 50).let { copy(minTenths = minOf(it, minTenths), maxTenths = it) }
    fun randomDelayMs(): Long = if (minTenths == maxTenths) minTenths * 100L
        else ThreadLocalRandom.current().nextLong(minTenths * 100L, maxTenths * 100L + 1)
    fun seconds(tenths: Int): String = String.format(Locale.ROOT, "%.1f", tenths / 10.0)
    val label: String get() = if (minTenths == maxTenths) "${seconds(minTenths)} 秒（固定）"
        else "${seconds(minTenths)}–${seconds(maxTenths)} 秒（随机）"
}

internal object AutoDelaySettings {
    fun read(context: Context): AutoDelayRange {
        val prefs = context.getSharedPreferences("unattended", Context.MODE_PRIVATE)
        val min = runCatching { prefs.getInt("delay_min_tenths", 20) }.getOrDefault(20).coerceIn(0, 50)
        val max = runCatching { prefs.getInt("delay_max_tenths", 50) }.getOrDefault(50).coerceIn(0, 50)
        return AutoDelayRange(minOf(min, max), maxOf(min, max))
    }

    fun save(context: Context, range: AutoDelayRange) {
        context.getSharedPreferences("unattended", Context.MODE_PRIVATE).edit()
            .putInt("delay_min_tenths", range.minTenths).putInt("delay_max_tenths", range.maxTenths).apply()
    }
}
