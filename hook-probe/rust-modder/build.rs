use std::{fs, io::Result, path::PathBuf};

use prost::Message;
use prost_types::FileDescriptorSet;

fn main() -> Result<()> {
    let descriptor = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../external/MajsoulMax-rs/liqi_config/liqi.desc");
    println!("cargo:rerun-if-changed={}", descriptor.display());

    let mut descriptor_set = FileDescriptorSet::decode(fs::read(descriptor)?.as_slice())
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
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
    config.compile_fds(descriptor_set)?;
    Ok(())
}
