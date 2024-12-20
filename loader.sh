#!/bin/bash

mkdir deps
cd deps
git clone https://github.com/seL4/seL4.git --config advice.detachedHead=false;
cd seL4
git checkout 4cae30a6ef166a378d4d23697b00106ce7e4e76f ;
cmake \
    -DCROSS_COMPILER_PREFIX=aarch64-linux-gnu- \
    -DCMAKE_TOOLCHAIN_FILE=gcc.cmake \
    -DCMAKE_INSTALL_PREFIX=install \
    -C ../../docker/kernel-settings.cmake \
    -G Ninja \
    -S . \
    -B build; \
ninja -C build all; \
ninja -C build install;
cd ..

export SEL4_INSTALL_DIR=$(pwd)/seL4/install
export SEL4_PREFIX=$SEL4_INSTALL_DIR
export CC=clang

curl -sSf https://sh.rustup.rs | \
        bash -s -- -y --no-modify-path \
            --default-toolchain nightly-2024-10-26\
            --component rust-src

cargo install \
    -Z build-std=core,alloc,compiler_builtins \
    -Z build-std-features=compiler-builtins-mem \
    --target aarch64-unknown-none \
    --git https://github.com/seL4/rust-sel4 \
	--rev "5b9ebfd0a3a9805f28cc9222cd558e8d56a3919d" \
    --root . \
    sel4-kernel-loader

cargo install --root . sel4-kernel-loader-add-payload --git https://github.com/seL4/rust-sel4 --rev "5b9ebfd0a3a9805f28cc9222cd558e8d56a3919d"

cd ..
