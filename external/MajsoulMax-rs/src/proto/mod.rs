pub mod base;

/// 由 `build.rs` 从 `liqi_config/liqi.desc` 生成，产物落在 `OUT_DIR`。
pub mod lq {
    include!(concat!(env!("OUT_DIR"), "/lq.rs"));
}
