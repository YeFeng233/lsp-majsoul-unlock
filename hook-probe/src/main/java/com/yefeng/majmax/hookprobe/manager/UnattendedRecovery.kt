package com.yefeng.majmax.hookprobe.manager

/** Wait for transient socket loss, then relaunch with bounded backoff. */
internal class UnattendedRecovery {
    private var disconnectedAt: Long? = null
    private var nextAttempt = 0L
    private var failures = 0

    fun shouldLaunch(enabled: Boolean, connected: Boolean, now: Long): Boolean {
        if (!enabled || connected) {
            disconnectedAt = null; nextAttempt = 0; failures = 0
            return false
        }
        val since = disconnectedAt ?: now.also { disconnectedAt = it }
        if (now - since < 10_000 || now < nextAttempt) return false
        nextAttempt = now + (30_000L shl failures.coerceAtMost(2))
        failures++
        return true
    }
}
