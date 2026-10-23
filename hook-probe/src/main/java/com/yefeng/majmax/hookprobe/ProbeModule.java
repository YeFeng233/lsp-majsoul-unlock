package com.yefeng.majmax.hookprobe;

import android.content.Context;
import android.os.Bundle;
import android.net.Uri;
import android.util.Log;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.util.Arrays;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.concurrent.Executors;
import java.util.concurrent.TimeUnit;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;
import org.json.JSONObject;
import com.yefeng.majmax.hookprobe.catalog.UnlockCatalogData;
import com.yefeng.majmax.hookprobe.catalog.UnlockCatalogInstaller;

import io.github.libxposed.api.XposedModule;

/** Modern API 102 entry for the in-process Rust Modder. */
public final class ProbeModule extends XposedModule {
    private static final String GAME = "com.soulgamechst.majsoul";
    private static final String TAG = "MajsoulProbe";
    private static final Uri AI_ENDPOINT_URI = Uri.parse(
            "content://com.yefeng.majmax.hookprobe.ai/capture");
    private static final String ASSET_VERSION = "0.11.0-catalog1";
    private static final String[] VERSIONED_ASSETS = {
            "ui/MajsoulMaxSettings.lua", "ui/MajsoulMaxAutoDiscard.lua"
    };
    private static final String[] USER_ASSETS = {"settings.mod.json"};
    private static final AtomicBoolean CONFIGURED = new AtomicBoolean();
    private static final AtomicBoolean AI_ENDPOINT_POLLING = new AtomicBoolean();
    private static final java.util.concurrent.ScheduledExecutorService AI_ENDPOINT_EXECUTOR =
            Executors.newSingleThreadScheduledExecutor(task -> {
                Thread thread = new Thread(task, "majmax-ai-endpoint");
                thread.setDaemon(true);
                return thread;
            });
    private static volatile int aiEndpointPort;
    private static volatile byte[] aiEndpointToken = new byte[0];
    private boolean targetProcess;

    private static native int nativeConfigure(String configDir);
    private static native void nativeCaptureEndpoint(int port, byte[] token);
    private static native void nativeAutoCommand(String command, long generation, long connection, long sequence);
    private static native String nativeAutoResults();

    @Override
    public void onModuleLoaded(ModuleLoadedParam param) {
        targetProcess = !param.isSystemServer() && GAME.equals(param.getProcessName());
        if (!targetProcess) return;
        HookDiagnostics.event("INFO", "hook.entry", "MODULE_LOADED",
                HookDiagnostics.fields("api", getApiVersion(), "moduleVersion", BuildConfig.VERSION_NAME));
        log(Log.INFO, TAG, "API=" + getApiVersion() + " framework="
                + getFrameworkName() + " " + getFrameworkVersion());
        try {
            System.loadLibrary("majsoulmodder");
            System.loadLibrary("majsoulprobe");
            HookDiagnostics.event("INFO", "hook.native", "NATIVE_LOADED", new JSONObject());
            log(Log.INFO, TAG, "Native Hook and Rust Modder loaded");
        } catch (Throwable error) {
            HookDiagnostics.event("ERROR", "hook.native", "NATIVE_LOAD_FAILED", new JSONObject());
            log(Log.ERROR, TAG, "Cannot load native Hook libraries", error);
        }
    }

    @Override
    public void onPackageReady(PackageReadyParam param) {
        if (!targetProcess || !GAME.equals(param.getPackageName()) || !param.isFirstPackage()) {
            return;
        }
        LspatchUnityCompat.install(this, GAME);
        try {
            Class<?> activity = param.getClassLoader().loadClass(
                    "com.soulgamechst.mahjongsoulsdk.MainActivity");
            Method onCreate = activity.getDeclaredMethod("onCreate", Bundle.class);
            hook(onCreate).intercept(chain -> {
                Object receiver = chain.getThisObject();
                if (receiver instanceof Context context) configure(context);
                return chain.proceed();
            });
            HookDiagnostics.event("INFO", "hook.entry", "ACTIVITY_HOOK_INSTALLED", new JSONObject());
            log(Log.INFO, TAG, "Game activity API 102 initialization hook installed");
        } catch (ReflectiveOperationException error) {
            HookDiagnostics.event("ERROR", "hook.entry", "ACTIVITY_HOOK_FAILED", new JSONObject());
            log(Log.ERROR, TAG, "Activity entry changed; Rust Modder cannot be configured", error);
        }
    }

    private void configure(Context gameContext) {
        if (!CONFIGURED.compareAndSet(false, true)) return;
        HookDiagnostics.start(gameContext);
        HookDiagnostics.event("INFO", "hook.entry", "CONFIG_STARTED", new JSONObject());
        try {
            File configDir = new File(gameContext.getFilesDir(), "majsoulmax-hook");
            if (!configDir.isDirectory() && !configDir.mkdirs()) {
                throw new IOException("Cannot create " + configDir);
            }
            UnlockCatalogData.Snapshot catalog = null;
            try (ZipFile moduleApk = new ZipFile(getModuleApplicationInfo().sourceDir)) {
                File stamp = new File(configDir, ".asset-version");
                String installed = stamp.isFile()
                        ? readUtf8(stamp).trim() : "";
                if (!ASSET_VERSION.equals(installed)) {
                    for (String name : VERSIONED_ASSETS) copyAsset(moduleApk, name, configDir, true);
                    writeUtf8(stamp, ASSET_VERSION);
                }
                for (String name : USER_ASSETS) copyAsset(moduleApk, name, configDir, false);
                // A UI asset upgrade must never replace a newer downloaded unlock list.
                copyAsset(moduleApk, "max_data.yaml", configDir, false);
                try { catalog = syncUnlockCatalog(gameContext, moduleApk, configDir); }
                catch (Exception error) {
                    HookDiagnostics.event("WARN", "hook.catalog", "CATALOG_UNAVAILABLE", new JSONObject());
                }
            }
            int result = nativeConfigure(configDir.getAbsolutePath());
            if (result != 0) throw new IOException("nativeConfigure returned " + result);
            if (catalog != null) reportCatalogApplied(gameContext, configDir, catalog);
            startAiEndpointDiscovery(gameContext.getApplicationContext());
            HookDiagnostics.event("INFO", "hook.entry", "CONFIGURED", new JSONObject());
            log(Log.INFO, TAG, "Rust Modder configured at " + configDir);
        } catch (Throwable error) {
            CONFIGURED.set(false);
            HookDiagnostics.event("ERROR", "hook.entry", "CONFIG_FAILED", new JSONObject());
            log(Log.ERROR, TAG, "Cannot configure Rust Modder; traffic remains unmodified", error);
        }
    }

    private static UnlockCatalogData.Snapshot syncUnlockCatalog(Context context, ZipFile apk, File directory) throws Exception {
        String manifest = catalogAsset(apk, "manifest.json", UnlockCatalogData.MAX_MANIFEST_BYTES);
        String payload = catalogAsset(apk, "unlock-data.json", UnlockCatalogData.MAX_DATA_BYTES);
        UnlockCatalogData.Snapshot chosen = UnlockCatalogData.parse(manifest, payload, BuildConfig.VERSION_CODE);
        try {
            Bundle reply = context.getContentResolver().call(AI_ENDPOINT_URI, "unlockCatalog", null, null);
            if (reply != null) {
                byte[] bytes = reply.getByteArray("payload");
                if (bytes == null || bytes.length > UnlockCatalogData.MAX_DATA_BYTES) throw new IOException("Invalid catalog payload");
                UnlockCatalogData.Snapshot downloaded = UnlockCatalogData.parse(
                        reply.getString("manifest", ""), UnlockCatalogData.utf8(bytes), BuildConfig.VERSION_CODE);
                if (downloaded.version > chosen.version || (downloaded.version == chosen.version && downloaded.sha256.equals(chosen.sha256))) {
                    chosen = downloaded;
                }
            }
        } catch (Exception ignored) {
            // Embedded modules and an unavailable manager retain their last good data.
        }
        boolean installed = UnlockCatalogInstaller.install(directory, chosen);
        HookDiagnostics.event("INFO", "hook.catalog", installed ? "CATALOG_INSTALLED" : "CATALOG_RETAINED",
                HookDiagnostics.fields("count", chosen.skinCount, "reason", "version_" + chosen.version));
        return chosen;
    }

    private static String catalogAsset(ZipFile apk, String name, int max) throws Exception {
        ZipEntry entry = apk.getEntry("assets/unlock-catalog/" + name);
        if (entry == null) throw new IOException("Missing bundled catalog");
        try (InputStream input = apk.getInputStream(entry)) {
            return UnlockCatalogData.utf8(UnlockCatalogData.readBounded(input, max));
        }
    }

    private static void reportCatalogApplied(Context context, File directory, UnlockCatalogData.Snapshot catalog) {
        try (InputStream input = new FileInputStream(new File(directory, "max_data.yaml"))) {
            String hash = UnlockCatalogData.sha256(UnlockCatalogData.readBounded(input, UnlockCatalogData.MAX_DATA_BYTES));
            if (!hash.equals(catalog.yamlSha256)) return;
            Bundle report = new Bundle();
            report.putLong("version", catalog.version); report.putString("sha256", hash);
            context.getContentResolver().call(AI_ENDPOINT_URI, "catalogApplied", null, report);
        } catch (Exception ignored) {}
    }

    private static void startAiEndpointDiscovery(Context gameContext) {
        if (!AI_ENDPOINT_POLLING.compareAndSet(false, true)) return;
        AI_ENDPOINT_EXECUTOR.scheduleWithFixedDelay(() -> {
            try {
                Bundle endpoint = gameContext.getContentResolver().call(
                        AI_ENDPOINT_URI, "endpoint", null, null);
                int port = endpoint == null ? 0 : endpoint.getInt("port", 0);
                byte[] token = endpoint == null ? null : endpoint.getByteArray("token");
                if (port > 0 && port <= 65535 && token != null && token.length == 32) {
                    if (port != aiEndpointPort || !Arrays.equals(token, aiEndpointToken)) {
                        nativeCaptureEndpoint(port, token);
                        HookDiagnostics.event("INFO", "hook.capture", "CAPTURE_ENDPOINT_READY", new JSONObject());
                        Arrays.fill(aiEndpointToken, (byte) 0);
                        aiEndpointPort = port;
                        aiEndpointToken = token.clone();
                    }
                } else if (aiEndpointPort != 0) {
                    nativeCaptureEndpoint(0, new byte[0]);
                    HookDiagnostics.event("INFO", "hook.capture", "CAPTURE_ENDPOINT_LOST", new JSONObject());
                    Arrays.fill(aiEndpointToken, (byte) 0);
                    aiEndpointPort = 0;
                    aiEndpointToken = new byte[0];
                }
                if (aiEndpointPort != 0) {
                    try {
                        Bundle report = new Bundle();
                        report.putString("acks", nativeAutoResults());
                        Bundle reply = gameContext.getContentResolver().call(AI_ENDPOINT_URI, "autoPoll", null, report);
                        String command = reply == null ? null : reply.getString("command");
                        JSONObject json = command == null ? null : new JSONObject(command);
                        nativeAutoCommand(command, json == null ? 0 : json.optLong("sourceGeneration"),
                                json == null ? 0 : Long.parseLong(json.optString("connection", "0")),
                                json == null ? 0 : json.optLong("sourceSequence"));
                    } catch (Throwable ignored) {
                        // Command polling is independent of passive capture. A bad
                        // command must never tear down the healthy game connection.
                        nativeAutoCommand(null, 0, 0, 0);
                    }
                } else {
                    nativeAutoCommand(null, 0, 0, 0);
                }
            } catch (Throwable ignored) {
                nativeAutoCommand(null, 0, 0, 0);
                if (aiEndpointPort != 0) {
                    nativeCaptureEndpoint(0, new byte[0]);
                    HookDiagnostics.event("WARN", "hook.capture", "CAPTURE_ENDPOINT_LOST", new JSONObject());
                    Arrays.fill(aiEndpointToken, (byte) 0);
                    aiEndpointPort = 0;
                    aiEndpointToken = new byte[0];
                }
            }
        }, 0, 250, TimeUnit.MILLISECONDS);
    }

    private static void copyAsset(ZipFile moduleApk, String name, File directory,
            boolean replace) throws IOException {
        File target = new File(directory, name);
        if (!replace && target.isFile()) return;
        File parent = target.getParentFile();
        if (parent != null && !parent.isDirectory() && !parent.mkdirs()) {
            throw new IOException("Cannot create asset directory " + parent);
        }
        File temporary = new File(directory, name + ".tmp");
        ZipEntry entry = moduleApk.getEntry("assets/liqi_config/" + name);
        if (entry == null) throw new IOException("Module asset is missing: " + name);
        try (InputStream input = moduleApk.getInputStream(entry);
             FileOutputStream output = new FileOutputStream(temporary, false)) {
            copy(input, output);
            output.getFD().sync();
        }
        if (target.exists() && !target.delete()) throw new IOException("Cannot replace " + target);
        if (!temporary.renameTo(target)) throw new IOException("Cannot install " + target);
    }

    private static String readUtf8(File file) throws IOException {
        try (FileInputStream input = new FileInputStream(file);
             ByteArrayOutputStream output = new ByteArrayOutputStream()) {
            copy(input, output);
            return output.toString(StandardCharsets.UTF_8.name());
        }
    }

    private static void writeUtf8(File file, String value) throws IOException {
        try (FileOutputStream output = new FileOutputStream(file, false)) {
            output.write(value.getBytes(StandardCharsets.UTF_8));
            output.getFD().sync();
        }
    }

    private static void copy(InputStream input, java.io.OutputStream output) throws IOException {
        byte[] buffer = new byte[8192];
        for (int count; (count = input.read(buffer)) != -1; ) {
            output.write(buffer, 0, count);
        }
    }
}
