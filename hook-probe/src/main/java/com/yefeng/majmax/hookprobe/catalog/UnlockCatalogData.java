package com.yefeng.majmax.hookprobe.catalog;

import org.json.JSONArray;
import org.json.JSONObject;
import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.nio.ByteBuffer;
import java.nio.charset.CodingErrorAction;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.util.Arrays;
import java.util.HashSet;
import java.util.Iterator;
import java.util.Set;
import java.util.TreeSet;

/** Data-only catalog: fixed numeric sections, never scripts or settings patches. */
public final class UnlockCatalogData {
    public static final int MAX_MANIFEST_BYTES = 16 * 1024;
    public static final int MAX_DATA_BYTES = 512 * 1024;
    public static final String[] SECTIONS = {"character", "skin", "title", "item", "loading_image", "emoji", "endings"};

    public static final class Snapshot {
        public final long version;
        public final String versionName, sha256, yamlSha256, yaml, manifest, payload;
        public final int skinCount, characterCount;
        private Snapshot(JSONObject info, JSONObject data, String manifest, String payload, String yaml) throws Exception {
            this.version = info.getLong("version");
            this.versionName = info.getString("versionName");
            this.sha256 = info.getString("sha256");
            this.yaml = yaml;
            this.yamlSha256 = sha256(yaml.getBytes(StandardCharsets.UTF_8));
            this.manifest = manifest;
            this.payload = payload;
            this.skinCount = data.getJSONArray("skin").length();
            this.characterCount = data.getJSONArray("character").length();
        }
        public String envelope() throws Exception {
            return new JSONObject().put("manifest", manifest).put("payload", payload).toString();
        }
    }

    public static JSONObject manifest(String text, int moduleVersion) throws Exception {
        require(text.getBytes(StandardCharsets.UTF_8).length <= MAX_MANIFEST_BYTES, "清单版本文件过大");
        JSONObject info = new JSONObject(text);
        require(number(info.get("schemaVersion"), false) == 1, "清单格式需要更新应用后才能读取");
        require(number(info.get("version"), false) <= 9_007_199_254_740_991L, "清单版本无效");
        long minimum = number(info.get("minModuleVersionCode"), false);
        require(minimum <= moduleVersion, "此清单需要更新应用后才能使用");
        require("unlock-data.json".equals(info.getString("dataFile")), "清单数据文件名无效");
        require(info.getString("sha256").matches("[a-f0-9]{64}"), "清单校验值无效");
        long size = number(info.get("size"), false);
        require(size <= MAX_DATA_BYTES, "清单数据过大");
        String name = info.getString("versionName");
        require(!name.trim().isEmpty() && name.length() <= 64 && !name.contains("\n"), "清单版本名称无效");
        return info;
    }

    public static Snapshot parse(String manifest, String payload, int moduleVersion) throws Exception {
        JSONObject info = manifest(manifest, moduleVersion);
        byte[] bytes = payload.getBytes(StandardCharsets.UTF_8);
        require(bytes.length == info.getLong("size") && bytes.length <= MAX_DATA_BYTES, "清单数据长度校验失败");
        require(sha256(bytes).equals(info.getString("sha256")), "清单数据 SHA-256 校验失败");
        JSONObject data = new JSONObject(payload);
        Set<String> keys = new HashSet<>();
        data.keys().forEachRemaining(keys::add);
        require(keys.equals(new HashSet<>(Arrays.asList(SECTIONS))), "清单数据栏目不完整或含未知栏目");
        StringBuilder yaml = new StringBuilder();
        int total = 0;
        for (String section : SECTIONS) {
            yaml.append(section).append(":\n");
            if (!section.equals("emoji")) {
                JSONArray values = data.getJSONArray(section);
                total += values.length();
                appendList(yaml, values, "");
            } else {
                JSONObject emojis = data.getJSONObject(section);
                require(emojis.length() > 0 && emojis.length() <= 2000, "表情角色清单长度无效");
                TreeSet<Long> characters = new TreeSet<>();
                Iterator<String> iterator = emojis.keys();
                while (iterator.hasNext()) {
                    String key = iterator.next();
                    require(key.matches("[1-9][0-9]*"), "表情角色 ID 无效");
                    long character = Long.parseLong(key);
                    require(character <= 0xffffffffL, "表情角色 ID 超出范围");
                    characters.add(character);
                }
                for (long character : characters) {
                    JSONArray values = emojis.getJSONArray(Long.toString(character));
                    total += values.length();
                    yaml.append("  ").append(character).append(":\n");
                    appendList(yaml, values, "  ");
                }
            }
        }
        require(total <= 50_000, "清单 ID 数量过多");
        return new Snapshot(info, data, manifest, payload, yaml.toString());
    }

    private static void appendList(StringBuilder yaml, JSONArray values, String indent) throws Exception {
        require(values.length() > 0 && values.length() <= 50_000, "清单列表长度无效");
        Set<Long> seen = new HashSet<>();
        for (int i = 0; i < values.length(); i++) {
            long value = id(values.get(i));
            require(seen.add(value), "清单包含重复 ID");
            yaml.append(indent).append("- ").append(value).append('\n');
        }
    }

    private static long id(Object value) throws Exception {
        long result = number(value, false);
        require(result <= 0xffffffffL, "清单 ID 超出范围");
        return result;
    }

    private static long number(Object value, boolean zero) throws Exception {
        require(value instanceof Number, "清单数字类型无效");
        String text = value.toString();
        require(text.matches(zero ? "[0-9]+" : "[1-9][0-9]*"), "清单数字无效");
        try { return Long.parseLong(text); }
        catch (NumberFormatException error) { throw new IllegalArgumentException("清单数字超出范围"); }
    }

    public static byte[] readBounded(InputStream input, int limit) throws Exception {
        ByteArrayOutputStream output = new ByteArrayOutputStream();
        byte[] buffer = new byte[8192];
        int count;
        while ((count = input.read(buffer)) != -1) {
            require(output.size() + count <= limit, "下载内容超过大小限制");
            output.write(buffer, 0, count);
        }
        return output.toByteArray();
    }

    public static String utf8(byte[] bytes) throws Exception {
        return StandardCharsets.UTF_8.newDecoder().onMalformedInput(CodingErrorAction.REPORT)
                .onUnmappableCharacter(CodingErrorAction.REPORT).decode(ByteBuffer.wrap(bytes)).toString();
    }

    public static String sha256(byte[] bytes) throws Exception {
        StringBuilder result = new StringBuilder();
        for (byte value : MessageDigest.getInstance("SHA-256").digest(bytes)) result.append(String.format("%02x", value & 255));
        return result.toString();
    }

    private static void require(boolean test, String message) {
        if (!test) throw new IllegalArgumentException(message);
    }
}
