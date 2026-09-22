pub mod base {
    #[derive(Clone, PartialEq, Eq, Hash, ::prost::Message)]
    pub struct BaseMessage {
        #[prost(string, tag = "1")]
        pub method_name: ::prost::alloc::string::String,
        #[prost(bytes = "vec", tag = "2")]
        pub data: ::prost::alloc::vec::Vec<u8>,
    }
}

pub mod lq {
    include!(concat!(env!("OUT_DIR"), "/lq.rs"));
}
