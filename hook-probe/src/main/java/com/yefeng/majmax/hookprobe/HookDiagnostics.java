package com.yefeng.majmax.hookprobe;

import android.content.Context;
import android.net.Uri;
import android.os.Bundle;

import org.json.JSONArray;
import org.json.JSONObject;

import java.io.File;
import java.nio.charset.StandardCharsets;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.List;
import java.util.UUID;
import java.util.concurrent.Executors;
import java.util.concurrent.ScheduledExecutorService;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicLong;

/** Small best-effort structured event queue; it never blocks a game callback on IPC or disk. */
final class HookDiagnostics {
    private static final Uri URI = Uri.parse("content://com.yefeng.majmax.hookprobe.ai/capture");
    private static final String SESSION = UUID.randomUUID().toString();
    private static final AtomicLong SEQUENCE = new AtomicLong();
    private static final ArrayDeque<JSONObject> QUEUE = new ArrayDeque<>();
    private static final ScheduledExecutorService WRITER = Executors.newSingleThreadScheduledExecutor(task -> {
        Thread thread = new Thread(task, "majmax-hook-diagnostics");
        thread.setDaemon(true);
        return thread;
    });
    private static volatile Context appContext;
    private static boolean scheduled;
    private static long dropped;

    private HookDiagnostics() {}

    static JSONObject fields(Object... pairs) {
        JSONObject result = new JSONObject();
        try {
            for (int i = 0; i + 1 < pairs.length; i += 2) result.put(String.valueOf(pairs[i]), pairs[i + 1]);
        } catch (Exception ignored) {}
        return result;
    }

    static synchronized void start(Context context) {
        appContext = context.getApplicationContext();
        loadSpool();
        if (!scheduled) {
            scheduled = true;
            WRITER.scheduleWithFixedDelay(HookDiagnostics::flush, 0, 2, TimeUnit.SECONDS);
        }
    }

    static synchronized void event(String level, String component, String code, JSONObject fields) {
        if (QUEUE.size() >= 128) {
            QUEUE.removeFirst();
            dropped++;
        }
        JSONObject record = new JSONObject();
        try {
            record.put("schema", 1);
            record.put("timeUtcMs", System.currentTimeMillis());
            record.put("session", SESSION);
            record.put("seq", SEQUENCE.incrementAndGet());
            record.put("level", level);
            record.put("component", component);
            record.put("code", code);
            record.put("fields", fields == null ? new JSONObject() : fields);
            QUEUE.addLast(record);
        } catch (Exception ignored) {
            return;
        }
        if (appContext != null && !scheduled) start(appContext);
    }

    private static void flush() {
        Context context = appContext;
        if (context == null) return;
        List<JSONObject> batch;
        synchronized (HookDiagnostics.class) {
            if (dropped > 0 && QUEUE.size() < 128) {
                long count = dropped;
                dropped = 0;
                event("WARN", "hook.capture", "LOG_DROPPED", fields("count", count));
            }
            if (QUEUE.isEmpty()) return;
            batch = new ArrayList<>(QUEUE);
            persist(context);
        }
        try {
            JSONArray records = new JSONArray();
            for (int i = 0; i < Math.min(64, batch.size()); i++) records.put(batch.get(i));
            if (records.toString().getBytes(StandardCharsets.UTF_8).length > 64 * 1024) return;
            Bundle extras = new Bundle();
            extras.putString("records", records.toString());
            Bundle reply = context.getContentResolver().call(URI, "hookDiagnostics", null, extras);
            long ack = reply == null ? -1 : reply.getLong("ackSeq", -1);
            if (ack < 0) return;
            synchronized (HookDiagnostics.class) {
                while (!QUEUE.isEmpty() && QUEUE.peekFirst().optLong("seq", Long.MAX_VALUE) <= ack) {
                    QUEUE.removeFirst();
                }
                persist(context);
            }
        } catch (Throwable ignored) {
            // Preserve the bounded private spool and retry; never propagate into a game callback.
        }
    }

    private static void loadSpool() {
        Context context = appContext;
        if (context == null) return;
        File spool = spool(context);
        if (!spool.isFile()) return;
        try {
            String text = new String(java.nio.file.Files.readAllBytes(spool.toPath()), StandardCharsets.UTF_8);
            JSONArray lines = new JSONArray(text);
            synchronized (HookDiagnostics.class) {
                for (int i = 0; i < lines.length() && QUEUE.size() < 128; i++) {
                    JSONObject item = lines.optJSONObject(i);
                    if (item != null && item.optInt("schema", -1) == 1) {
                        QUEUE.addLast(item);
                        SEQUENCE.set(Math.max(SEQUENCE.get(), item.optLong("seq", 0)));
                    }
                }
            }
        } catch (Throwable ignored) { spool.delete(); }
    }

    private static void persist(Context context) {
        try {
            JSONArray items = new JSONArray();
            for (JSONObject item : QUEUE) items.put(item);
            File target = spool(context);
            File temp = new File(context.getFilesDir(), "hook-diagnostics.tmp");
            java.nio.file.Files.write(temp.toPath(), items.toString().getBytes(StandardCharsets.UTF_8));
            try {
                java.nio.file.Files.move(temp.toPath(), target.toPath(),
                        java.nio.file.StandardCopyOption.REPLACE_EXISTING,
                        java.nio.file.StandardCopyOption.ATOMIC_MOVE);
            } catch (java.nio.file.AtomicMoveNotSupportedException ignored) {
                java.nio.file.Files.move(temp.toPath(), target.toPath(),
                        java.nio.file.StandardCopyOption.REPLACE_EXISTING);
            }
        } catch (Throwable ignored) {}
    }

    private static File spool(Context context) {
        return new File(context.getFilesDir(), "hook-diagnostics.json");
    }
}
