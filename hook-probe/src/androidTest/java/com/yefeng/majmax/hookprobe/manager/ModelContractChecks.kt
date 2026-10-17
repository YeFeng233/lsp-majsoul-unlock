package com.yefeng.majmax.hookprobe.manager

import android.content.Context
import android.net.Uri
import java.io.File

/** Exercises real ONNX Runtime, JNI and model storage with deterministic tiny graphs. */
internal object ModelContractChecks {
    fun run(target: Context, tests: Context) {
        val folder = File(target.cacheDir, "contract-fixtures").apply { mkdirs() }
        fun model(name: String): File = File(folder,name).also { file ->
            tests.assets.open("model-fixtures/$name").use { input ->
                file.outputStream().use { output -> input.copyTo(output) }
            }
        }
        fun import(name: String) = OnnxModelStore.import(target,Uri.fromFile(model(name)))
        check(import("akagi-renamed.onnx").format == "akagi-onnx")
        check(runCatching { import("akagi-no-contract.onnx") }.isFailure)
        check(runCatching { import("mortal-sanma.onnx") }.isFailure)
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
        OnnxModelStore.selectBuiltIn(target,4)
        OnnxModelStore.closeAll()
        target.getSharedPreferences("onnx-policy-models",Context.MODE_PRIVATE).edit().clear().commit()
    }
}
