fn main() {
    // Tell Cargo that if the given file changes, to rerun this build script.
    let libary_dir = env!("BUILD_DIR");
    let libc_dir = env!("LIBC_DIR");

    println!("cargo:rustc-link-search={}", libary_dir);
    println!("cargo:rustc-link-search={}", libc_dir);
    println!("cargo::rustc-link-lib=static=fat",);
    println!("cargo::rustc-link-lib=static=sddf_util_serial",);
    println!("cargo::rustc-link-lib=static=lib_sddf_lwip",);
    println!("cargo::rustc-link-lib=static=c",);
}
