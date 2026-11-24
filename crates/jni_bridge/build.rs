//! Links the NDK camera library (ndk-sys only links libandroid / libmediandk).
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        println!("cargo:rustc-link-lib=camera2ndk");
    }
}
