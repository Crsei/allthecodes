#![allow(dead_code, unused_imports)]

#[macro_use]
#[path = "src/macros.rs"]
mod macros;

#[path = "src/codegen.rs"]
mod codegen;
#[path = "src/error.rs"]
mod error;
#[path = "src/notification.rs"]
mod notification;
#[path = "src/request.rs"]
mod request;
#[path = "src/response.rs"]
mod response;
#[path = "src/v1/mod.rs"]
mod v1;

fn main() {
    println!("cargo:rerun-if-changed=src/codegen.rs");
    println!("cargo:rerun-if-changed=src/error.rs");
    println!("cargo:rerun-if-changed=src/macros.rs");
    println!("cargo:rerun-if-changed=src/notification.rs");
    println!("cargo:rerun-if-changed=src/request.rs");
    println!("cargo:rerun-if-changed=src/response.rs");
    println!("cargo:rerun-if-changed=src/v1");

    let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") else {
        return;
    };
    let manifest_dir = std::path::Path::new(&manifest_dir);
    let frontend_path = codegen::default_frontend_types_path(manifest_dir);

    if frontend_path.parent().is_some_and(std::path::Path::exists) {
        if let Err(error) = codegen::write_frontend_types(manifest_dir) {
            println!("cargo:warning=failed to generate frontend API types: {error}");
        }
    }
}
