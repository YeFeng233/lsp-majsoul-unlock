package com.yefeng.majmax.hookprobe;

import android.content.Context;
import android.os.Bundle;
import android.util.Log;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;

import io.github.libxposed.api.XposedModule;

/** Modern API 102 entry for the in-process Rust Modder. */
public final class ProbeModule extends XposedModule {
    private static final String GAME = "com.soulgamechst.majsoul";
    private static final String TAG = "MajsoulProbe";
    private static final String ASSET_VERSION = "0.4.3";
    private static final String[] VERSIONED_ASSETS = {
            "max_data.yaml", "ui/MajsoulMaxSettings.lua"
    };
    private static final String[] USER_ASSETS = {"settings.mod.json"};
    private static final AtomicBoolean CONFIGURED = new AtomicBoolean();
    private boolean targetProcess;

    private static native int nativeConfigure(String configDir);

    @Override
    public void onModuleLoaded(ModuleLoadedParam param) {
        targetProcess = !param.isSystemServer() && GAME.equals(param.getProcessName());
        if (!targetProcess) return;
        log(Log.INFO, TAG, "API=" + getApiVersion() + " framework="
                + getFrameworkName() + " " + getFrameworkVersion());
        try {
            System.loadLibrary("majsoulmodder");
            System.loadLibrary("majsoulprobe");
            log(Log.INFO, TAG, "Native Hook and Rust Modder loaded");
        } catch (Throwable error) {
            log(Log.ERROR, TAG, "Cannot load native Hook libraries", error);
        }
    }

    @Override
    public void onPackageReady(PackageReadyParam param) {
        if (!targetProcess || !GAME.equals(param.getPackageName()) || !param.isFirstPackage()) {
            return;
        }
        try {
            Class<?> activity = param.getClassLoader().loadClass(
                    "com.soulgamechst.mahjongsoulsdk.MainActivity");
            Method onCreate = activity.getDeclaredMethod("onCreate", Bundle.class);
            hook(onCreate).intercept(chain -> {
                Object receiver = chain.getThisObject();
                if (receiver instanceof Context context) configure(context);
                return chain.proceed();
            });
            log(Log.INFO, TAG, "Game activity API 102 initialization hook installed");
        } catch (ReflectiveOperationException error) {
            log(Log.ERROR, TAG, "Activity entry changed; Rust Modder cannot be configured", error);
        }
    }

    private void configure(Context gameContext) {
        if (!CONFIGURED.compareAndSet(false, true)) return;
        try {
            File configDir = new File(gameContext.getFilesDir(), "majsoulmax-hook");
            if (!configDir.isDirectory() && !configDir.mkdirs()) {
                throw new IOException("Cannot create " + configDir);
            }
            try (ZipFile moduleApk = new ZipFile(getModuleApplicationInfo().sourceDir)) {
                File stamp = new File(configDir, ".asset-version");
                String installed = stamp.isFile()
                        ? readUtf8(stamp).trim() : "";
                if (!ASSET_VERSION.equals(installed)) {
                    for (String name : VERSIONED_ASSETS) copyAsset(moduleApk, name, configDir, true);
                    writeUtf8(stamp, ASSET_VERSION);
                }
                for (String name : USER_ASSETS) copyAsset(moduleApk, name, configDir, false);
            }
            int result = nativeConfigure(configDir.getAbsolutePath());
            if (result != 0) throw new IOException("nativeConfigure returned " + result);
            log(Log.INFO, TAG, "Rust Modder configured at " + configDir);
        } catch (Throwable error) {
            CONFIGURED.set(false);
            log(Log.ERROR, TAG, "Cannot configure Rust Modder; traffic remains unmodified", error);
        }
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
