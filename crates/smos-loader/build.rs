fn main() {
    let linker_script = env!("LOADER_LINKER_SCRIPT");
    println!("cargo::rustc-link-arg=-T{}", linker_script);
}
