#!/bin/bash

git clone https://github.com/seL4/seL4.git --config advice.detachedHead=false;
cd seL4
git checkout 9bac64c6ceb1ece54fe00eae44065a836bd224f3;
cmake \
    -DCROSS_COMPILER_PREFIX=aarch64-linux-gnu- \
    -DCMAKE_TOOLCHAIN_FILE=gcc.cmake \
    -DCMAKE_INSTALL_PREFIX=install \
    -C ../docker/kernel-settings.cmake \
    -G Ninja \
    -S . \
    -B build; \
ninja -C build all; \
ninja -C build install;
cd ..

export SEL4_INSTALL_DIR=$(pwd)/seL4/install
export SEL4_PREFIX=$SEL4_INSTALL_DIR
export CC=clang

cargo install \
    -Z build-std=core,alloc,compiler_builtins \
    -Z build-std-features=compiler-builtins-mem \
    --target aarch64-unknown-none \
    --git https://github.com/seL4/rust-sel4 \
    --root . \
    sel4-kernel-loader

cargo install --root . sel4-kernel-loader-add-payload --git https://github.com/seL4/rust-sel4
