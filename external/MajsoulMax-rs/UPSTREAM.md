# Pinned MajsoulMax-rs components

Source: https://github.com/Xerxes-2/MajsoulMax-rs

Pinned commit: `7065716d12514b0a6a4bbc55adf29c0b5b2bacaa`.

This directory keeps the upstream Modder source, Liqi descriptor and runtime configuration needed by the Android hook. The Android-specific C ABI and settings adapter live in `hook-probe/rust-modder`. The upstream project is licensed under GPL-3.0; its license is retained here.

## Android catalog additions, 2026-09-24

`liqi_config/max_data.yaml` adds the following entries from the game's 2026-09-23 content update. The character and skin IDs were found in the connected Android client's runtime tables. Names and decoration IDs were cross-checked against [MajsoulData's extracted item tables](https://github.com/Avenshy/MajsoulData/tree/2677b55ad27dd4103f786c9dee33779d38b30f60/data/tables/item_definition) and its `0.16.283-4.0.47` catalog release. Existing entries and the pinned Modder/protocol sources are preserved.

| Entry | Section | ID |
| --- | --- | --- |
| 墨菲 | character | 20000126 |
| 墨菲默认服装 | skin | 40012601 |
| 墨菲契约服装 | skin | 40012602 |
| 和牌-灵感大暴走 | item | 30520015 |
| 立直-给点颜色 | item | 30530015 |
| 立直棒-墨彩喷喷 | item | 30560012 |
| 桌布-混一色！ | item | 30580030 |
| 牌背-喵体力学 | item | 30570019 |
| 守梦夜明-2026（限时物品） | loading_image | 307295 |
| 守梦夜明-2026（普通物品） | loading_image | 307495 |
| 守梦夜明-2026（插画配置） | loading_image | 260901 |

The Android asset revision is bumped so a future APK containing this catalog replaces the game's cached catalog while preserving the user's settings. These are local display overrides, not changes to server-side ownership.

## Local persistence fix, 2026-09-27

`src/modder.rs` saves the settings document immediately after handling `.lq.Lobby.changeCharacterSkin`, before sending the local character notification. The pinned upstream handler only updates `char_skin` in memory, so the selected skin is otherwise lost on process restart unless another settings action happens to save it. The Android settings adapter provides the existing atomic file writer. The regression test in `hook-probe/rust-modder` checks immediate persistence, repeated changes, unrelated settings and restoration from disk when the server returns default skins after restart.

The adapter names temporary settings files using the process ID and a write sequence, rather than thread names that can contain invalid path characters on the Windows test host.

## Additional skin catalog entries, 2026-09-27

The connected client's shop shows four Drowsy Daylight (纯白午后) skins that were missing from the local catalog. Their IDs were present in the Android runtime table inspected on September 24 and are cross-checked against the same pinned MajsoulData tables linked above. This update adds only those four skin IDs and refreshes the Android asset revision while keeping the user's selected skins.

| Character | Character ID | Skin ID |
| --- | --- | --- |
| 相原舞 | 200005 | 400509 |
| 如月彩音 | 200068 | 406805 |
| 二之宫花 | 200017 | 401710 |
| 雏桃 | 200026 | 402607 |
