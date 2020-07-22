fn main() {
    hrpc_build::compile_protos(&["src/schema.proto"], &["src"]).unwrap();
}
