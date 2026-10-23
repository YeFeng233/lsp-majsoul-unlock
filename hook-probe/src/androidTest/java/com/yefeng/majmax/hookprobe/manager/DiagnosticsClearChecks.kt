package com.yefeng.majmax.hookprobe.manager

import android.content.Context
import android.content.ContextWrapper
import java.io.File
import java.util.UUID
import org.json.JSONArray
import org.json.JSONObject

/** Exercise real file deletion without touching the manager's own diagnostics. */
internal object DiagnosticsClearChecks {
    fun run(realTarget: Context) {
        val root = File(realTarget.cacheDir, "diagnostics-clear-${UUID.randomUUID()}").apply { mkdirs() }
        val target = object : ContextWrapper(realTarget) {
            override fun getFilesDir(): File = root
        }
        try {
            val unrelated = File(root, "policy.pth").apply { writeText("keep model") }
            val legacy = File(root, "manager-diagnostics.log").apply {
                writeText("1|INFO|manager|legacy event\n")
            }
            val hook = JSONObject().put("schema", 1).put("timeUtcMs", 1)
                .put("session", UUID.randomUUID().toString()).put("seq", 1)
                .put("level", "INFO").put("component", "hook.entry").put("code", "MODULE_LOADED")
            check(DiagnosticsStore.acceptHookBatch(target, JSONArray().put(hook).toString()) == 1L)
            DiagnosticsStore.appendAssistant(target, "INFO", "manager", "TEST_EVENT")
            val directory = File(root, "diagnostics")
            // Retained rotations must also be removed, not just the active file.
            for (kind in DiagnosticKind.entries) {
                val active = File(directory, "${kind.filename}-1.jsonl")
                for (index in 2..4) active.copyTo(File(directory, "${kind.filename}-$index.jsonl"))
            }
            val hookBefore = DiagnosticsStore.read(target, DiagnosticKind.Hook).size
            DiagnosticsStore.clear(target, DiagnosticKind.Assistant)
            check(DiagnosticsStore.read(target, DiagnosticKind.Assistant).isEmpty())
            check(DiagnosticsStore.read(target, DiagnosticKind.Hook).size == hookBefore)
            check(!legacy.exists())
            for (index in 1..4) check(!File(directory, "assistant-$index.jsonl").exists())
            check(unrelated.readText() == "keep model")

            DiagnosticsStore.appendAssistant(target, "INFO", "manager", "AFTER_CLEAR")
            check(DiagnosticsStore.read(target, DiagnosticKind.Assistant).single().code == "AFTER_CLEAR")
            DiagnosticsStore.clear(target, DiagnosticKind.Hook)
            check(DiagnosticsStore.read(target, DiagnosticKind.Hook).isEmpty())
            check(DiagnosticsStore.read(target, DiagnosticKind.Assistant).size == 1)
            DiagnosticsStore.clear(target)
            DiagnosticsStore.clear(target) // Repeated cleanup of an empty store is safe.
            check(DiagnosticKind.entries.all { DiagnosticsStore.read(target, it).isEmpty() })
            check(unrelated.exists())

            // A legacy-only installation must not resurrect cleared records on read.
            File(root, "manager-diagnostics.migrated").delete()
            legacy.writeText("2|INFO|manager|second legacy event\n")
            DiagnosticsStore.clear(target)
            check(DiagnosticsStore.read(target, DiagnosticKind.Assistant).isEmpty())
        } finally {
            root.deleteRecursively()
        }
    }
}
