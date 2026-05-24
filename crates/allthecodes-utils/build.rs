//! Build script — link platform libs needed by transitive deps.
//!
//! libgit2-sys (via our `git2` dep) uses Windows CryptoAPI hash functions
//! that live in `advapi32.dll`. The libgit2-sys build script emits the
//! link directive, but rust-lld on MSVC only picks it up for the final
//! binary, not for test binaries built from leaf crates. Emitting the
//! directive here guarantees cc-utils' own test binary links cleanly
//! without depending on a heavier parent crate to pull Advapi32 in.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-lib=dylib=advapi32");
    }
}
