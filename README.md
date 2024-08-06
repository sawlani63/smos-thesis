# SMOS-rs

## Description

A rewrite of the Secure Multiserver Operating System (SMOS) project using rust-seL4.

# How to build/run

## Dependencies

seL4 host dependencies: https://docs.sel4.systems/projects/buildsystem/host-dependencies.html
meson-build
`qemu-system-aarch64`
Rust toolchain: Installed by loader.sh script

## Building

./loader.sh
meson setup --cross-file meson-toolchain.txt build
meson compile -C build

## Running (QEMU-only)
```
./run-qemu
```
