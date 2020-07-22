use std::path::PathBuf;

pub mod codegen {
    include!(concat!(env!("OUT_DIR"), "/hyperspace.rs"));
}

pub use codegen::*;

const SOCKET_NAME: &str = "hyperspace";

pub fn socket_path<T>(socket_name: Option<T>) -> PathBuf
where
    T: ToString,
{
    let socket_name = socket_name
        .map(|t| t.to_string())
        .unwrap_or(SOCKET_NAME.into());
    std::env::temp_dir().join(format!("{}.sock", socket_name))
}
