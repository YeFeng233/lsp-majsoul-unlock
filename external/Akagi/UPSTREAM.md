# Pinned Akagi components

Source: https://github.com/shinkuan/Akagi

Commit: `81f530639cf10740ca1876df827d3c08efbe1175` (v3, 3.7.1).

Only `native_bot`, the Majsoul bridge/protobuf schema, game-state tracking,
analysis, and their shared data types are vendored. Files in these directories
are unmodified upstream source. `hook-probe/akagi-core` supplies the small
Android adapter; the desktop Tauri application, proxy, browser automation,
cloud API, and automatic game input are not included.

The bundled four-player and three-player models are Akagi's small native
behavior-cloned models, not the Mortal model. Weight files and feature
encoding must be updated together. Preserve `LICENSE.txt` and `NOTICE` when
redistributing this code or its weights.
