// 雀魂 WebSocket 的外层信封类型。
//
// 原先由 prost-build 从 proto/basic.proto 生成，该文件在适配 Unity 新版数据时
// 一并移除了（liqi.desc 里不含此包）。这个结构极小且稳定，故改为手工维护。
#[derive(Clone, PartialEq, Eq, Hash, ::prost::Message)]
pub struct BaseMessage {
    #[prost(string, tag = "1")]
    pub method_name: ::prost::alloc::string::String,
    #[prost(bytes = "vec", tag = "2")]
    pub data: ::prost::alloc::vec::Vec<u8>,
}
