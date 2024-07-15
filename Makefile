# Copyright 2023, Colias Group, LLC
#
# SPDX-License-Identifier: BSD-2-Clause
#

BUILD ?= $(abspath build)

build_dir := $(BUILD)

SEL4_INSTALL_DIR := $(shell pwd)/deps/seL4/install
sel4_prefix := $(SEL4_INSTALL_DIR)

# Kernel loader binary artifacts provided by Docker container:
# - `sel4-kernel-loader`: The loader binary, which expects to have a payload appended later via
#   binary patch.
# - `sel4-kernel-loader-add-payload`: CLI which appends a payload to the loader.
loader_artifacts_dir := deps/bin/
loader := $(loader_artifacts_dir)/sel4-kernel-loader
loader_cli := $(loader_artifacts_dir)/sel4-kernel-loader-add-payload

.PHONY: none
none:

.PHONY: clean
clean:
	rm -rf $(build_dir)

# Build the system initalizer
init_crate := init
init := $(build_dir)/$(init_crate).elf
$(init): $(init).intermediate

.INTERMDIATE: $(init).intermediate
$(init).intermediate:
	SEL4_PREFIX=$(sel4_prefix) \
		cargo build \
			-Z build-std=core,alloc,compiler_builtins \
			-Z build-std-features=compiler-builtins-mem \
			-Z unstable-options \
			--target support/targets/aarch64-sel4.json \
			--target-dir $(abspath $(build_dir)/target) \
			--out-dir $(build_dir) \
			-p $(init_crate)

# Build the eth driver
eth_driver_crate := eth_driver
eth_driver := $(build_dir)/$(eth_driver_crate).elf
$(eth_driver): $(eth_driver).intermediate

.INTERMDIATE: $(eth_driver).intermediate
$(eth_driver).intermediate:
	SEL4_PREFIX=$(sel4_prefix) \
		cargo build \
			-Z build-std=core,alloc,compiler_builtins \
			-Z build-std-features=compiler-builtins-mem \
			-Z unstable-options \
			--target support/targets/aarch64-sel4.json \
			--target-dir $(abspath $(build_dir)/target) \
			--out-dir $(build_dir) \
			-p $(eth_driver_crate)


# Build the boot file server
bfs_crate := boot_file_server
bfs := $(build_dir)/$(bfs_crate).elf
$(bfs): $(bfs).intermediate

.INTERMDIATE: $(bfs).intermediate
$(bfs).intermediate: $(init) $(eth_driver)
	SEL4_PREFIX=$(sel4_prefix) \
	INIT_ELF=$(init) \
	ETH_DRIVER_ELF=$(eth_driver) \
		cargo build \
			-Z build-std=core,alloc,compiler_builtins \
			-Z build-std-features=compiler-builtins-mem \
			-Z unstable-options \
			--target support/targets/aarch64-sel4.json \
			--target-dir $(abspath $(build_dir)/target) \
			--out-dir $(build_dir) \
			-p $(bfs_crate)

# Build the loader
smos_loader_crate := smos-loader
smos_loader := $(build_dir)/$(smos_loader_crate).elf
$(smos_loader): $(smos_loader).intermediate

.INTERMDIATE: $(smos_loader).intermediate
$(smos_loader).intermediate:
	SEL4_PREFIX=$(sel4_prefix) \
	LINKER_SCRIPT=$(shell pwd)/crates/smos-loader/custom.ld \
		cargo build \
			-Z build-std=core,alloc,compiler_builtins \
			-Z build-std-features=compiler-builtins-mem \
			-Z unstable-options \
			--target support/targets/aarch64-sel4.json \
			--target-dir $(abspath $(build_dir)/target) \
			--out-dir $(build_dir) \
			-p $(smos_loader_crate)

# Build the root server
root_server_crate := root_server
root_server := $(build_dir)/$(root_server_crate).elf
$(root_server): $(root_server).intermediate

# SEL4_TARGET_PREFIX is used by build.rs scripts of various rust-sel4 crates to locate seL4
# configuration and libsel4 headers.
.INTERMDIATE: $(root_server).intermediate
$(root_server).intermediate: $(bfs) $(smos_loader)
	SEL4_PREFIX=$(sel4_prefix) \
	BOOT_FS_ELF=$(bfs) \
	LOADER_ELF=$(smos_loader) \
		cargo build \
			--verbose \
			-Z build-std=core,alloc,compiler_builtins \
			-Z build-std-features=compiler-builtins-mem \
			-Z unstable-options \
			--target support/targets/aarch64-sel4.json \
			--target-dir $(abspath $(build_dir)/target) \
			--out-dir $(build_dir) \
			-p $(root_server_crate)


image := $(build_dir)/image.elf
# Append the payload to the loader using the loader CLI
$(image): $(root_server)
	$(loader_cli) \
		--loader $(loader) \
		--sel4-prefix $(sel4_prefix) \
		--app $(root_server) \
		-o $@

qemu_cmd := \
	qemu-system-aarch64 \
		-machine virt,virtualization=on -cpu cortex-a57 -m size=2G \
		-serial mon:stdio \
		-nographic \
		-kernel $(image)

# 		-s -S \

.PHONY: run
run: $(image)
	$(qemu_cmd)

.PHONY: test
test: test.py $(image)
	python3 $< $(qemu_cmd)
