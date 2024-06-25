fn main() {
	let linker_script = env!("LINKER_SCRIPT");
	println!("cargo::rustc-link-arg=-T{}", linker_script);
}