plugins {
    id("com.android.application") version "8.7.3"
    id("org.jetbrains.kotlin.android") version "2.0.21"
    id("org.jetbrains.kotlin.plugin.compose") version "2.0.21"
}

val upstreamDir = rootProject.projectDir.resolve("../external/MajsoulMax-rs")
val stagedAssets = layout.buildDirectory.dir("generated/upstreamAssets")
val updateOwner = providers.gradleProperty("updateOwner").orElse("YeFeng233").get()
val updateRepo = providers.gradleProperty("updateRepo").orElse("lsp-majsoul-unlock").get()
val updateChannel = providers.gradleProperty("updateChannel").orElse("stable").get()
val buildSha = providers.gradleProperty("buildSha")
    .orElse(providers.environmentVariable("GITHUB_SHA").map { it.take(7) })
    .orElse("local build").get()
val distributionAbis = mapOf(
    "arm64" to listOf("arm64-v8a"),
    "universal" to listOf("arm64-v8a", "armeabi-v7a", "x86", "x86_64"),
)
val stageUpstreamAssets by tasks.registering(Copy::class) {
    from(upstreamDir.resolve("liqi_config")) {
        include("max_data.yaml", "settings.mod.json")
        into("liqi_config")
    }
    from(rootProject.projectDir.resolve("../external/Akagi/LICENSE.txt")) {
        into("licenses")
        rename { "Akagi-LICENSE.txt" }
    }
    from(rootProject.projectDir.resolve("../external/Akagi/NOTICE")) {
        into("licenses")
        rename { "Akagi-NOTICE.txt" }
    }
    into(stagedAssets)
}

android {
    namespace = "com.yefeng.majmax.hookprobe"
    compileSdk = 35
    buildToolsVersion = "35.0.0"

    // Pin the debug signer explicitly so CI and local builds use the same key.
    signingConfigs {
        create("persistentDebug") {
            storeFile = file(providers.environmentVariable("ANDROID_DEBUG_KEYSTORE_PATH")
                .orElse("${System.getProperty("user.home")}/.android/debug.keystore").get())
            storePassword = "android"
            keyAlias = "androiddebugkey"
            keyPassword = "android"
        }
    }

    defaultConfig {
        applicationId = "com.yefeng.majmax.hookprobe"
        minSdk = 29
        targetSdk = 35
        versionCode = 16
        versionName = "0.7.2"
        buildConfigField("String", "UPDATE_OWNER", "\"${updateOwner.replace("\"", "\\\"")}\"")
        buildConfigField("String", "UPDATE_REPO", "\"${updateRepo.replace("\"", "\\\"")}\"")
        buildConfigField("String", "UPDATE_CHANNEL", "\"${updateChannel.replace("\"", "\\\"")}\"")
        buildConfigField("String", "BUILD_SHA", "\"${buildSha.replace("\"", "\\\"")}\"")
    }

    flavorDimensions += "distribution"
    productFlavors {
        distributionAbis.forEach { (name, abis) ->
            create(name) {
                dimension = "distribution"
                ndk { abiFilters += abis }
            }
        }
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    buildTypes {
        getByName("debug") {
            signingConfig = signingConfigs.getByName("persistentDebug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    sourceSets["main"].jniLibs.srcDir(layout.buildDirectory.dir("native-libs"))
    sourceSets["main"].assets.srcDir(stageUpstreamAssets)
    // LSPosed's module classloader resolves libraries inside the APK.
    // Keep them uncompressed so Android can map them directly from the ZIP.
    packaging.jniLibs.useLegacyPackaging = false
}

dependencies {
    compileOnly("io.github.libxposed:api:102.0.0")

    implementation(platform("androidx.compose:compose-bom:2024.12.01"))
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("com.microsoft.onnxruntime:onnxruntime-android:1.30.0")
    debugImplementation("androidx.compose.ui:ui-tooling")
}

tasks.named("preBuild") {
    dependsOn(stageUpstreamAssets)
}

distributionAbis.forEach { (distribution, abis) ->
    val flavorName = distribution.replaceFirstChar { it.uppercaseChar() }
    val verifyNative = tasks.register("verify${flavorName}NativeLibraries") {
        doLast {
            val root = layout.buildDirectory.dir("native-libs").get().asFile
            val required = listOf("libmajsoulprobe.so", "libmajsoulmodder.so",
                "libmajsoulai.so", "libmajsoulai_jni.so")
            val missing = abis.flatMap { abi -> required.map { "$abi/$it" } }
                .filterNot { root.resolve(it).isFile }
            check(missing.isEmpty()) {
                "Missing native libraries: ${missing.joinToString()}. " +
                    "Run build-native.ps1 -NdkPath <NDK> -Abis ${if (distribution == "universal") "all" else "arm64-v8a"}"
            }
        }
    }
    tasks.configureEach {
        if (name.startsWith("pre$flavorName") && name.endsWith("Build")) {
            dependsOn(verifyNative)
        }
    }
}
