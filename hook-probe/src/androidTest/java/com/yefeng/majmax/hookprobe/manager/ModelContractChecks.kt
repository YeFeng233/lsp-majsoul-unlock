package com.yefeng.majmax.hookprobe.manager

import android.content.Context
import android.content.ContextWrapper
import android.net.Uri
import java.io.File

/** Exercises real ONNX Runtime, JNI and model storage with deterministic tiny graphs. */
internal object ModelContractChecks {
    fun run(realTarget: Context, tests: Context) {
        val root = File(realTarget.cacheDir, "isolated-model-checks").apply { mkdirs() }
        val target = object : ContextWrapper(realTarget) {
            override fun getApplicationContext(): Context = this
            override fun getFilesDir(): File = File(root,"files").apply { mkdirs() }
            override fun getCacheDir(): File = File(root,"cache").apply { mkdirs() }
            override fun getSharedPreferences(name: String, mode: Int) = super.getSharedPreferences("model-checks-$name",mode)
        }
        try {
            val folder = File(target.cacheDir, "contract-fixtures").apply { mkdirs() }
            fun model(name: String): File = File(folder,name).also { file ->
                tests.assets.open("model-fixtures/$name").use { input ->
                    file.outputStream().use { output -> input.copyTo(output) }
                }
            }
            fun import(name: String) = OnnxModelStore.import(target,Uri.fromFile(model(name)))
            check(import("akagi-renamed.onnx").format == "akagi-onnx")
            val importedLog = DiagnosticsStore.read(target, DiagnosticKind.Assistant).last { it.code == "MODEL_IMPORTED" }
            val importId = importedLog.fields.getString("importId")
            val importLogs = DiagnosticsStore.read(target, DiagnosticKind.Assistant)
                .filter { it.fields.optString("importId") == importId }
            check(importLogs.first().code == "MODEL_IMPORT_STARTED")
            check(importLogs.any { it.code == "MODEL_IMPORT_FILE_COPIED" && it.fields.getLong("bytes") > 0 })
            check(importLogs.any { it.code == "MODEL_IMPORT_VALIDATED" })
            check(importedLog.fields.getString("stage") == "complete")
            check(importedLog.fields.getLong("elapsedMs") >= 0)
            // Failures before file copying must also appear in the exported diagnostics.
            val unsupported = File(folder, "unsupported.txt").apply { writeText("invalid") }
            check(runCatching { OnnxModelStore.import(target, Uri.fromFile(unsupported)) }.isFailure)
            val rejected = DiagnosticsStore.read(target, DiagnosticKind.Assistant).last { it.code == "MODEL_IMPORT_REJECTED" }
            check(rejected.fields.getString("stage") == "extension")
            check(rejected.fields.getString("errorClass") == "IllegalArgumentException")
            val brokenPth = File(folder, "broken.pth").apply { writeText("invalid zip") }
            check(runCatching { OnnxModelStore.import(target, Uri.fromFile(brokenPth)) }.isFailure)
            val archiveFailure = DiagnosticsStore.read(target, DiagnosticKind.Assistant).last { it.code == "MODEL_IMPORT_REJECTED" }
            check(archiveFailure.fields.getInt("nativeStatus") == -7)
            check(archiveFailure.fields.getString("reason") == "invalid_pth_archive")
            java.util.zip.ZipFile(DiagnosticsStore.packageZip(target, null)).use { zip ->
                val exported = zip.getInputStream(zip.getEntry("assistant.jsonl")).bufferedReader().use { it.readText() }
                check(exported.contains(importId) && exported.contains("invalid_pth_archive"))
                check(!exported.contains(folder.absolutePath))
            }
            check(runCatching { import("akagi-no-contract.onnx") }.isFailure)
            check(import("mortal-sanma.onnx").players == 3)
            check(runCatching { import("mortal-sanma-wrong-actions.onnx") }.isFailure)
            check(runCatching { import("mortal-four-wrong-actions.onnx") }.isFailure)
            check(runCatching { import("mortal-old.onnx") }.isFailure)
            for (name in listOf("mortal-bool.onnx","mortal-float.onnx")) {
                val policy = import(name)
                check(policy.players == 4 && policy.format == "mortal-onnx")
                OnnxModelStore.prepareSelected(target)
                val input = FloatArray(1012*34+46)
                input[1012*34+4] = 1f; input[1012*34+7] = 1f
                val result = checkNotNull(OnnxModelStore.infer(4,input))
                check(result[4] == 4f && result[7] == 7f)
                check(result.indices.filter { it != 4 && it != 7 }.all { result[it] == 0f })
                check(OnnxModelStore.infer(4,FloatArray(1012*34)) == null)
            }
            for (name in listOf("mortal-sanma-bool.onnx","mortal-sanma-float.onnx")) {
                val policy = import(name)
                check(policy.players == 3 && policy.format == "mortal-onnx")
                OnnxModelStore.prepareSelected(target)
                val input = FloatArray(775*34+44)
                input[775*34+35] = 1f; input[775*34+40] = 1f
                val result = checkNotNull(OnnxModelStore.infer(3,input))
                check(result.size == 44 && result[35] == 35f && result[40] == 40f)
                check(result.indices.filter { it != 35 && it != 40 }.all { result[it] == 0f })
                check(OnnxModelStore.infer(3,FloatArray(775*34)) == null)
            }
            OnnxModelStore.selectBuiltIn(target,4)
            OnnxModelStore.selectBuiltIn(target,3)
        } finally {
            OnnxModelStore.closeAll()
            target.getSharedPreferences("onnx-policy-models",Context.MODE_PRIVATE).edit().clear().commit()
            AiNative.configureCustomContract(3,0,1,37,60)
            AiNative.configureCustomContract(4,0,1,39,82)
            OnnxModelStore.initialize(realTarget)
            root.deleteRecursively()
        }
    }
}
