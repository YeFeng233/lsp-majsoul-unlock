package com.yefeng.majmax.hookprobe.catalog;

import android.util.AtomicFile;
import java.io.File;
import java.io.FileOutputStream;
import java.nio.charset.StandardCharsets;
import org.json.JSONObject;

/** Next-start installation only: never resets Rust state during an active game. */
public final class UnlockCatalogInstaller {
    public static boolean install(File directory, UnlockCatalogData.Snapshot snapshot) throws Exception {
        File data = new File(directory, "max_data.yaml");
        AtomicFile stamp = new AtomicFile(new File(directory, ".unlock-catalog.json"));
        long installed = 0;
        String hash = "";
        try (var input = stamp.openRead()) {
            JSONObject info = new JSONObject(UnlockCatalogData.utf8(UnlockCatalogData.readBounded(input, 2048)));
            installed = info.getLong("version");
            hash = info.getString("sha256");
            if (installed <= 0 || !hash.matches("[a-f0-9]{64}")) { installed = 0; hash = ""; }
        } catch (Exception ignored) {}
        String actual = "";
        if (data.isFile()) try (var input = new java.io.FileInputStream(data)) {
            actual = UnlockCatalogData.sha256(UnlockCatalogData.readBounded(input, UnlockCatalogData.MAX_DATA_BYTES));
        } catch (Exception ignored) {}
        if (installed > snapshot.version && hash.equals(actual)) return false;
        if (installed == snapshot.version && snapshot.yamlSha256.equals(actual) && hash.equals(actual)) return false;
        write(new AtomicFile(data), snapshot.yaml);
        write(stamp, new JSONObject().put("version", snapshot.version).put("sha256", snapshot.yamlSha256).toString());
        return true;
    }

    private static void write(AtomicFile file, String text) throws Exception {
        FileOutputStream output = file.startWrite();
        try {
            output.write(text.getBytes(StandardCharsets.UTF_8));
            file.finishWrite(output);
        } catch (Throwable error) {
            file.failWrite(output);
            throw error;
        }
    }
}
