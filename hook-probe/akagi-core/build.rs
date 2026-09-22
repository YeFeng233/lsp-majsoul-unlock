fn main() {
    let proto = "../../external/Akagi/src/bridge/majsoul/proto";
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    prost_build::Config::new()
        .file_descriptor_set_path(output.join("liqi_desc.bin"))
        .compile_protos(&[format!("{proto}/liqi.proto")], &[proto])
        .expect("compile pinned Akagi Liqi descriptors");
    println!("cargo:rerun-if-changed={proto}/liqi.proto");
}
