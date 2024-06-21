fn main() {
	println!("cargo::rustc-link-arg=-T/Users/alwinjoshy/work/smos_new/smos-rs/crates/smos-loader/custom.ld");
	// println!("cargo::rustc-link-arg=-Tdata=2000");
	// println!("cargo::rustc-link-arg=-Ttext=8000");
	// println!("cargo::rustc-link-arg=--section-start=.init=0x200000");
	// println!("cargo::rustc-link-arg=--section-start=.note.gnu.build-id=0x202000");
}