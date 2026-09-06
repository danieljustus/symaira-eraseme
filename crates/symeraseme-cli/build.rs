fn main() {
    // CARGO_PKG_VERSION is deterministic package metadata; no clock or VCS
    // state is included in the public version contract.
    println!(
        "cargo:rustc-env=SYMERASEME_VERSION={}",
        env!("CARGO_PKG_VERSION")
    );
    println!("cargo:rerun-if-changed=build.rs");
}
