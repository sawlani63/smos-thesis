fn main() {
    // Tell Cargo that if the given file changes, to rerun this build script.
    let libary_dir = env!("BUILD_DIR");
    println!("cargo:rustc-link-search={}", libary_dir);
    println!("cargo::rustc-link-lib=static=copy",);
    println!("cargo::rustc-link-lib=static=sddf_util",);
}
