# SMOS-rs

## Description

A rewrite of the Secure Multiserver Operating System (SMOS) project using rust-seL4.

# How to build/run

## Dependencies

* seL4 Host Dependencies: https://docs.sel4.systems/projects/buildsystem/host-dependencies.html
    * Base dependencies
    * Python dependencies
    * QEMU-system-aarch64
    * AArch64 cross-compiler
* meson-build
* Rust toolchain: Installed by loader.sh script
* aarch64-none-elf toolchain (confirmed to be working with 13.3 and 12.3) from https://developer.arm.com/downloads/-/arm-gnu-toolchain-downloads

## Building
```
./loader.sh
meson setup --cross-file meson-toolchain.txt build
meson compile -C build
```
## Running (QEMU-only)
```
./run-qemu
```
