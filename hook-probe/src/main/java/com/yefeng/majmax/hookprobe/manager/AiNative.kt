package com.yefeng.majmax.hookprobe.manager

/** Loaded only in the independent manager process. */
object AiNative {
    init {
        System.loadLibrary("majsoulai")
        System.loadLibrary("majsoulai_jni")
    }
    @JvmStatic external fun reset()
    @JvmStatic external fun configurePolicy(mask: Int)
    @JvmStatic external fun loadMortalPth(expectedPlayers: Int, path: String, activate: Boolean): Int
    @JvmStatic external fun configureCustomContract(players: Int, kind: Int, version: Int, inChannels: Int, actionSpace: Int)
    @JvmStatic external fun frame(connection: Long, direction: Int, bytes: ByteArray): String?
    @JvmStatic external fun selfTest(): String
}
