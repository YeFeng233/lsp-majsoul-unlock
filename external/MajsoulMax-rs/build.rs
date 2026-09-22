use std::{fs, io::Result};

use prost::Message;
use prost_types::FileDescriptorSet;

fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=liqi_config/liqi.desc");

    let descriptor_bytes = fs::read("liqi_config/liqi.desc")?;
    let mut descriptor_set = FileDescriptorSet::decode(descriptor_bytes.as_slice())
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    // The modder only uses lq protocol types. The other packages describe
    // legacy resource tables that max_data.yaml replaces.
    descriptor_set
        .file
        .retain(|file| file.package.as_deref() == Some("lq"));

    let mut config = prost_build::Config::new();
    config
        .type_attribute(".", "#[allow(dead_code)]")
        .type_attribute(
            "lq.ViewSlot",
            "#[derive(::serde::Serialize, ::serde::Deserialize)]",
        );
    // 输出到 OUT_DIR 而非 src/proto，避免生成代码污染 cargo fmt / clippy 的检查范围
    config.compile_fds(descriptor_set)?;

    Ok(())
}
