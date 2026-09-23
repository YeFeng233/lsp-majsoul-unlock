package com.yefeng.majmax.hookprobe.manager

import android.content.ContentProvider
import android.content.ContentValues
import android.content.pm.PackageManager
import android.database.Cursor
import android.net.Uri
import android.os.Binder
import android.os.Bundle

/** Delivers a temporary loopback capability only to the game UID. */
class AiEndpointProvider : ContentProvider() {
    override fun onCreate() = true

    override fun call(method: String, arg: String?, extras: Bundle?): Bundle? {
        val app = checkNotNull(context)
        val gameUid = try {
            app.packageManager.getApplicationInfo(AiOverlayService.GAME, 0).uid
        } catch (_: PackageManager.NameNotFoundException) {
            throw SecurityException("Target game is not installed")
        }
        if (Binder.getCallingUid() != gameUid) {
            throw SecurityException("Only the target game may request the local AI endpoint")
        }
        if (method == "hookDiagnostics") {
            val payload = extras?.getString("records") ?: throw IllegalArgumentException("Missing diagnostics batch")
            val ack = DiagnosticsStore.acceptHookBatch(app, payload)
            return Bundle().apply { putLong("ackSeq", ack) }
        }
        if (method != "endpoint") return null
        val endpoint = AiOverlayService.currentEndpoint()
        return Bundle().apply {
            putInt("port", endpoint?.port ?: 0)
            endpoint?.let { putByteArray("token", it.token) }
        }
    }

    override fun query(uri: Uri, projection: Array<out String>?, selection: String?,
        selectionArgs: Array<out String>?, sortOrder: String?): Cursor? = null

    override fun getType(uri: Uri): String? = null
    override fun insert(uri: Uri, values: ContentValues?): Uri? = null
    override fun delete(uri: Uri, selection: String?, selectionArgs: Array<out String>?): Int = 0
    override fun update(uri: Uri, values: ContentValues?, selection: String?,
        selectionArgs: Array<out String>?): Int = 0
}

internal object DiagnosticsAccess {
    private val uri = Uri.parse("content://com.yefeng.majmax.hookprobe.ai/capture")
    fun grantToGame(context: android.content.Context) {
        runCatching { context.grantUriPermission(AiOverlayService.GAME, uri,
            android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION) }
    }
}
