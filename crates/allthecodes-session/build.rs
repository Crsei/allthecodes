//! Build script — link Windows advapi32 for libgit2-sys CryptoAPI. Mirrors
//! the shim added to cc-utils (both crates pull in `git2`, and rust-lld on
//! MSVC only picks up libgit2-sys's link directive for final binaries).

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-lib=dylib=advapi32");
    }
}
