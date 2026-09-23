package com.yefeng.majmax.hookprobe;

import android.content.Context;
import android.os.ParcelFileDescriptor;
import android.system.Os;
import android.system.OsConstants;
import android.util.Log;

import java.io.File;
import java.io.FileDescriptor;
import java.io.IOException;
import java.lang.reflect.Method;
import java.util.concurrent.atomic.AtomicBoolean;

import io.github.libxposed.api.XposedModule;

/** Keeps Unity's installation-path query compatible with LSPatch's cached original APK. */
final class LspatchUnityCompat {
    private static final String TAG = "MajsoulProbe";
    private final XposedModule module;
    private final String packageName;
    private final AtomicBoolean reportedFailure = new AtomicBoolean();
    // Hold the directory descriptor for the lifetime of the process. The APK itself
    // is still opened normally by Unity, using the filename below this directory.
    private ParcelFileDescriptor originDirectory;
    private String unityCodePath;

    private LspatchUnityCompat(XposedModule module, String packageName) {
        this.module = module;
        this.packageName = packageName;
    }

    static void install(XposedModule module, String packageName) {
        LspatchUnityCompat compat = new LspatchUnityCompat(module, packageName);
        try {
            Method method = Class.forName("android.app.ContextImpl")
                    .getDeclaredMethod("getPackageCodePath");
            module.hook(method).intercept(chain -> {
                Object original = chain.proceed();
                if (!(original instanceof String path)
                        || !path.contains("/cache/lspatch/origin/")
                        || !(chain.getThisObject() instanceof Context context)
                        || !packageName.equals(context.getPackageName())
                        || !isUnityNativeRender()) {
                    return original;
                }
                return compat.codePath(context, path);
            });
        } catch (ReflectiveOperationException | RuntimeException error) {
            module.log(Log.WARN, TAG, "Cannot install LSPatch Unity compatibility hook", error);
        }
    }

    private static boolean isUnityNativeRender() {
        for (StackTraceElement frame : Thread.currentThread().getStackTrace()) {
            if (frame.isNativeMethod()
                    && "com.unity3d.player.UnityPlayer".equals(frame.getClassName())
                    && "nativeRender".equals(frame.getMethodName())) {
                return true;
            }
        }
        return false;
    }

    private synchronized String codePath(Context context, String original) {
        if (unityCodePath != null) return unityCodePath;
        ParcelFileDescriptor directory = null;
        try {
            File apk = new File(original);
            File expected = new File(context.getCacheDir(), "lspatch/origin");
            if (!apk.getName().endsWith(".apk") || !apk.isFile() || !expected.isDirectory()
                    || !expected.getCanonicalFile().equals(apk.getParentFile().getCanonicalFile())) {
                return original;
            }
            FileDescriptor descriptor = Os.open(expected.getPath(),
                    OsConstants.O_RDONLY | OsConstants.O_CLOEXEC, 0);
            try {
                directory = ParcelFileDescriptor.dup(descriptor);
            } finally {
                Os.close(descriptor);
            }
            String alias = "/proc/self/fd/" + directory.getFd() + "/" + apk.getName();
            if (!new File(alias).isFile()) throw new IOException("Original APK alias is unreadable");

            // Unity rejects /data/user paths. A process-local directory descriptor gives
            // it a readable alias to the SAME original ZIP, including the .apk suffix.
            // Returning the outer patched base.apk breaks asset reads because its linked
            // ZIP entries have different offsets. Leave the other resource paths alone.
            originDirectory = directory;
            unityCodePath = alias;
            directory = null;
            module.log(Log.INFO, TAG, "LSPatch Unity APK alias: " + original + " -> " + alias);
            HookDiagnostics.event("INFO", "hook.compat", "LSPATCH_UNITY_APK_ALIAS_READY",
                    HookDiagnostics.fields("package", packageName));
            return alias;
        } catch (Exception error) {
            if (reportedFailure.compareAndSet(false, true)) {
                module.log(Log.WARN, TAG, "Cannot prepare original APK alias for Unity", error);
            }
            return original;
        } finally {
            if (directory != null) {
                try {
                    directory.close();
                } catch (IOException ignored) {
                }
            }
        }
    }
}
