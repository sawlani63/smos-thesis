# Copyright 2023, Colias Group, LLC
#
# SPDX-License-Identifier: BSD-2-Clause
#

BUILD ?= $(abspath build)
BUILD_DIR := $(BUILD)
SDDF := $(abspath sddf)
ETH_DRIVER := $(SDDF)/drivers/network/virtio
TIMER_DRIVER := $(SDDF)/drivers/clock/arm
ETH_COMPONENTS := $(SDDF)/network/components
# LWIPDIR := $(SDDF)/network/ipstacks/lwip/src
LWIPDIR := sddf/network/ipstacks/lwip/src
ECHO_SERVER := sddf/examples/echo_server

SEL4_INSTALL_DIR := $(shell pwd)/deps/seL4/install
sel4_prefix := $(SEL4_INSTALL_DIR)

BLK_DRIVER := $(SDDF)/drivers/blk/virtio

#FS := fs/fat

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
	rm -rf $(BUILD_DIR)

# Build the system initalizer
init_crate := init
init := $(BUILD_DIR)/$(init_crate).elf
$(init): $(init).intermediate

.INTERMDIATE: $(init).intermediate
$(init).intermediate:
	SEL4_PREFIX=$(sel4_prefix) \
		cargo build \
			-Z build-std=core,alloc,compiler_builtins \
			-Z build-std-features=compiler-builtins-mem \
			-Z unstable-options \
			--target support/targets/aarch64-sel4.json \
			--target-dir $(abspath $(BUILD_DIR)/target) \
			--out-dir $(BUILD_DIR) \
			-p $(init_crate)


# Build the C ethernet driver component

# TOOLCHAIN := clang
# CC1 := clang
# TARGET := aarch64-none-elf
# AR := llvm-ar

CC1 := aarch64-none-elf-gcc
AR := aarch64-none-elf-ar
LD := aarch64-none-elf-ld

# 	-target $(TARGET) \

CFLAGS1 := \
	-mstrict-align \
	-ffreestanding \
	-Wno-unused-function \
	-I$(SDDF)/include \
	-I$(SEL4_INSTALL_DIR)/libsel4/include \
	-I$(LWIPDIR)/include \
	-I$(LWIPDIR)/include/ipv4 \
	-I$(ECHO_SERVER)/include/lwip \

# Build static library for fat_fs
#$(BUILD_DIR)/decl.o: $(FS)/decl.h
#	$(CC1) -c $(CFLAGS1) $(FS)/decl.h -o $(BUILD_DIR)/decl.o

#$(BUILD_DIR)/event.o: $(FS)/event.c
#	$(CC1) -c $(CFLAGS1) $(FS)/event.c -o $(BUILD_DIR)/event.o

#$(BUILD_DIR)/io.o: $(FS)/io.c
#	$(CC1) -c $(CFLAGS1) $(FS)/io.c -o $(BUILD_DIR)/io.o

#$(BUILD_DIR)/op.o: $(FS)/op.c
#	$(CC1) -c $(CFLAGS1) $(FS)/op.c -o $(BUILD_DIR)/op.o

#$(BUILD_DIR)/libfat.a: $(BUILD_DIR)/decl.o $(BUILD_DIR)/event.o $(BUILD_DIR)/io.o $(BUILD_DIR)/op.o
#	$(AR) rvcs $@ $(BUILD_DIR)/decl.o $(BUILD_DIR)/event.o $(BUILD_DIR)/io.o $(BUILD_DIR)/op.o

# Build static library for sddf_util
$(BUILD_DIR)/printf.o: $(SDDF)/util/printf.c
	$(CC1) -c $(CFLAGS1) $(SDDF)/util/printf.c -o $(BUILD_DIR)/printf.o

$(BUILD_DIR)/assert.o: $(SDDF)/util/assert.c
	$(CC1) -c $(CFLAGS1) $(SDDF)/util/assert.c -o $(BUILD_DIR)/assert.o

$(BUILD_DIR)/sddf_putchar.o: $(SDDF)/util/putchar_debug.c
	$(CC1) -c $(CFLAGS1) $(SDDF)/util/putchar_debug.c -o $(BUILD_DIR)/sddf_putchar.o

$(BUILD_DIR)/cache.o: $(SDDF)/util/cache.c
	$(CC1) -c $(CFLAGS1) $(SDDF)/util/cache.c -o $(BUILD_DIR)/cache.o

$(BUILD_DIR)/libsddf_util.a: $(BUILD_DIR)/printf.o $(BUILD_DIR)/sddf_putchar.o $(BUILD_DIR)/assert.o $(BUILD_DIR)/cache.o
	${AR} rvcs $@ $(BUILD_DIR)/printf.o $(BUILD_DIR)/sddf_putchar.o $(BUILD_DIR)/assert.o $(BUILD_DIR)/cache.o

# Build static library for ethernet driver
$(BUILD_DIR)/ethernet.o: $(ETH_DRIVER)/ethernet.c $(ETH_DRIVER)/ethernet.h
	$(CC1) -c $(CFLAGS1) $(ETH_DRIVER)/ethernet.c -o $(BUILD_DIR)/ethernet.o

$(BUILD_DIR)/libethernet.a: $(BUILD_DIR)/ethernet.o
	${AR} rvcs $@ $(BUILD_DIR)/ethernet.o

# Build static library for blk driver
$(BUILD_DIR)/block.o: $(BLK_DRIVER)/block.c $(BLK_DRIVER)/block.h
	$(CC1) -c $(CFLAGS1) $(BLK_DRIVER)/block.c -o $(BUILD_DIR)/block.o

$(BUILD_DIR)/libblk_driver.a: $(BUILD_DIR)/block.o
	${AR} rvcs $@ $(BUILD_DIR)/block.o

# Build static library for rx_virt
$(BUILD_DIR)/virt_rx.o: $(ETH_COMPONENTS)/virt_rx.c
	$(CC1) -c $(CFLAGS1) $(ETH_COMPONENTS)/virt_rx.c -o $(BUILD_DIR)/virt_rx.o

$(BUILD_DIR)/libvirt_rx.a: $(BUILD_DIR)/virt_rx.o
	${AR} rvcs $@ $(BUILD_DIR)/virt_rx.o

# Build static library for tx_virt
$(BUILD_DIR)/virt_tx.o: $(ETH_COMPONENTS)/virt_tx.c
	$(CC1) -c $(CFLAGS1) $(ETH_COMPONENTS)/virt_tx.c -o $(BUILD_DIR)/virt_tx.o

$(BUILD_DIR)/libvirt_tx.a: $(BUILD_DIR)/virt_tx.o
	${AR} rvcs $@ $(BUILD_DIR)/virt_tx.o

# Build static library for copy
$(BUILD_DIR)/copy.o: $(ETH_COMPONENTS)/copy.c
	$(CC1) -c $(CFLAGS1) $(ETH_COMPONENTS)/copy.c -o $(BUILD_DIR)/copy.o

$(BUILD_DIR)/libcopy.a: $(BUILD_DIR)/copy.o
	${AR} rvcs $@ $(BUILD_DIR)/copy.o


# Build static library for timer
$(BUILD_DIR)/timer.o: $(TIMER_DRIVER)/timer.c
	$(CC1) -c $(CFLAGS1) $^ -o $@

$(BUILD_DIR)/libtimer.a: $(BUILD_DIR)/timer.o
	${AR} rvcs $@ $(BUILD_DIR)/timer.o

# Build static library for echo server
$(BUILD_DIR)/newlibc.o: $(SDDF)/util/newlibc.c
	$(CC1) -c $(CFLAGS1) $(SDDF)/util/newlibc.c -o $(BUILD_DIR)/newlibc.o


COREFILES=$(LWIPDIR)/core/init.c \
	$(LWIPDIR)/core/def.c \
	$(LWIPDIR)/core/dns.c \
	$(LWIPDIR)/core/inet_chksum.c \
	$(LWIPDIR)/core/ip.c \
	$(LWIPDIR)/core/mem.c \
	$(LWIPDIR)/core/memp.c \
	$(LWIPDIR)/core/netif.c \
	$(LWIPDIR)/core/pbuf.c \
	$(LWIPDIR)/core/raw.c \
	$(LWIPDIR)/core/stats.c \
	$(LWIPDIR)/core/sys.c \
	$(LWIPDIR)/core/altcp.c \
	$(LWIPDIR)/core/altcp_alloc.c \
	$(LWIPDIR)/core/altcp_tcp.c \
	$(LWIPDIR)/core/tcp.c \
	$(LWIPDIR)/core/tcp_in.c \
	$(LWIPDIR)/core/tcp_out.c \
	$(LWIPDIR)/core/timeouts.c \
	$(LWIPDIR)/core/udp.c

CORE4FILES=$(LWIPDIR)/core/ipv4/autoip.c \
	$(LWIPDIR)/core/ipv4/dhcp.c \
	$(LWIPDIR)/core/ipv4/etharp.c \
	$(LWIPDIR)/core/ipv4/icmp.c \
	$(LWIPDIR)/core/ipv4/igmp.c \
	$(LWIPDIR)/core/ipv4/ip4_frag.c \
	$(LWIPDIR)/core/ipv4/ip4.c \
	$(LWIPDIR)/core/ipv4/ip4_addr.c

APIFILES=$(LWIPDIR)/api/err.c

# NETIFFILES: Files implementing various generic network interface functions
NETIFFILES=$(LWIPDIR)/netif/ethernet.c

LWIPFILES=$(ECHO_SERVER)/lwip.c $(ECHO_SERVER)/udp_echo_socket.c $(COREFILES) $(CORE4FILES) $(NETIFFILES) $(APIFILES)
LWIP_OBJS := $(LWIPFILES:.c=.o) newlibc.o

$(BUILD_DIR)/%.d $(BUILD_DIR)/%.o: %.c Makefile
	mkdir -p `dirname $(BUILD_DIR)/$*.o`
	$(CC1) -c $(CFLAGS1) $< -o $(BUILD_DIR)/$*.o

$(BUILD_DIR)/libecho_server.a: $(addprefix $(BUILD_DIR)/, $(LWIP_OBJS))
	${AR} rcvs $@ $^

# Build the fat fs
#fat_crate := fat
#fat := $(BUILD_DIR)/$(fat_crate).elf
#$(fat): $(fat).intermediate

#.INTERMDIATE: $(fat).intermediate
#$(fat).intermediate: $(BUILD_DIR)/libfat.a
#	BUILD_DIR=$(BUILD_DIR) \
#	SEL4_PREFIX=$(sel4_prefix) \
#		cargo build \
#			-Z build-std=core,alloc,compiler_builtins \
#			-Z build-std-features=compiler-builtins-mem \
#			-Z unstable-options \
#			--target support/targets/aarch64-sel4.json \
#			--target-dir $(abspath $(BUILD_DIR)/target) \
#			--out-dir $(BUILD_DIR) \
#			-p $(fat_crate)

# Build the eth driver
eth_driver_crate := eth_driver
eth_driver := $(BUILD_DIR)/$(eth_driver_crate).elf
$(eth_driver): $(eth_driver).intermediate

.INTERMDIATE: $(eth_driver).intermediate
$(eth_driver).intermediate: $(BUILD_DIR)/libethernet.a $(BUILD_DIR)/libsddf_util.a
	BUILD_DIR=$(BUILD_DIR) \
	SEL4_PREFIX=$(sel4_prefix) \
		cargo build \
			-Z build-std=core,alloc,compiler_builtins \
			-Z build-std-features=compiler-builtins-mem \
			-Z unstable-options \
			--target support/targets/aarch64-sel4.json \
			--target-dir $(abspath $(BUILD_DIR)/target) \
			--out-dir $(BUILD_DIR) \
			-p $(eth_driver_crate)

# Build the blk driver
blk_driver_crate := blk_driver
blk_driver := $(BUILD_DIR)/$(blk_driver_crate).elf
$(blk_driver): $(blk_driver).intermediate

.INTERMDIATE: $(blk_driver).intermediate
$(blk_driver).intermediate: $(BUILD_DIR)/libblk_driver.a $(BUILD_DIR)/libsddf_util.a
	BUILD_DIR=$(BUILD_DIR) \
	SEL4_PREFIX=$(sel4_prefix) \
		cargo build \
			-Z build-std=core,alloc,compiler_builtins \
			-Z build-std-features=compiler-builtins-mem \
			-Z unstable-options \
			--target support/targets/aarch64-sel4.json \
			--target-dir $(abspath $(BUILD_DIR)/target) \
			--out-dir $(BUILD_DIR) \
			-p $(blk_driver_crate)

# Build the eth rx_virt
eth_virt_rx_crate := eth_virt_rx
eth_virt_rx := $(BUILD_DIR)/$(eth_virt_rx_crate).elf
$(eth_virt_rx): $(eth_virt_rx).intermediate

.INTERMDIATE: $(eth_virt_rx).intermediate
$(eth_virt_rx).intermediate: $(BUILD_DIR)/libvirt_rx.a $(BUILD_DIR)/libsddf_util.a
	BUILD_DIR=$(BUILD_DIR) \
	SEL4_PREFIX=$(sel4_prefix) \
		cargo build \
			-Z build-std=core,alloc,compiler_builtins \
			-Z build-std-features=compiler-builtins-mem \
			-Z unstable-options \
			--target support/targets/aarch64-sel4.json \
			--target-dir $(abspath $(BUILD_DIR)/target) \
			--out-dir $(BUILD_DIR) \
			-p $(eth_virt_rx_crate)


# Build the eth tx_virt
eth_virt_tx_crate := eth_virt_tx
eth_virt_tx := $(BUILD_DIR)/$(eth_virt_tx_crate).elf
$(eth_virt_tx): $(eth_virt_tx).intermediate

.INTERMDIATE: $(eth_virt_tx).intermediate
$(eth_virt_tx).intermediate: $(BUILD_DIR)/libvirt_tx.a $(BUILD_DIR)/libsddf_util.a
	BUILD_DIR=$(BUILD_DIR) \
	SEL4_PREFIX=$(sel4_prefix) \
		cargo build \
			-Z build-std=core,alloc,compiler_builtins \
			-Z build-std-features=compiler-builtins-mem \
			-Z unstable-options \
			--target support/targets/aarch64-sel4.json \
			--target-dir $(abspath $(BUILD_DIR)/target) \
			--out-dir $(BUILD_DIR) \
			-p $(eth_virt_tx_crate)

# Build the eth copier
eth_copier_crate := eth_copier
eth_copier := $(BUILD_DIR)/$(eth_copier_crate).elf
$(eth_copier): $(eth_copier).intermediate

.INTERMDIATE: $(eth_copier).intermediate
$(eth_copier).intermediate: $(BUILD_DIR)/libcopy.a $(BUILD_DIR)/libsddf_util.a
	BUILD_DIR=$(BUILD_DIR) \
	SEL4_PREFIX=$(sel4_prefix) \
		cargo build \
			-Z build-std=core,alloc,compiler_builtins \
			-Z build-std-features=compiler-builtins-mem \
			-Z unstable-options \
			--target support/targets/aarch64-sel4.json \
			--target-dir $(abspath $(BUILD_DIR)/target) \
			--out-dir $(BUILD_DIR) \
			-p $(eth_copier_crate)

# Build the timer
timer_crate := timer
timer := $(BUILD_DIR)/$(timer_crate).elf
$(timer): $(timer).intermediate

.INTERMDIATE: $(timer).intermediate
$(timer).intermediate: $(BUILD_DIR)/libtimer.a $(BUILD_DIR)/libsddf_util.a
	BUILD_DIR=$(BUILD_DIR) \
	SEL4_PREFIX=$(sel4_prefix) \
		cargo build \
			-Z build-std=core,alloc,compiler_builtins \
			-Z build-std-features=compiler-builtins-mem \
			-Z unstable-options \
			--target support/targets/aarch64-sel4.json \
			--target-dir $(abspath $(BUILD_DIR)/target) \
			--out-dir $(BUILD_DIR) \
			-p $(timer_crate)

# Build the echo server
LIBC := $(dir $(realpath $(shell aarch64-none-elf-gcc --print-file-name libc.a)))

echo_server_crate := echo_server
echo_server := $(BUILD_DIR)/$(echo_server_crate).elf
$(echo_server): $(echo_server).intermediate

.INTERMDIATE: $(echo_server).intermediate
$(echo_server).intermediate: $(BUILD_DIR)/libecho_server.a $(BUILD_DIR)/libsddf_util.a
	BUILD_DIR=$(BUILD_DIR) \
	LIBC_DIR=$(LIBC) \
	SEL4_PREFIX=$(sel4_prefix) \
		cargo build \
			-Z build-std=core,alloc,compiler_builtins \
			-Z build-std-features=compiler-builtins-mem \
			-Z unstable-options \
			--target support/targets/aarch64-sel4.json \
			--target-dir $(abspath $(BUILD_DIR)/target) \
			--out-dir $(BUILD_DIR) \
			-p $(echo_server_crate)

# Build the boot file server
bfs_crate := boot_file_server
bfs := $(BUILD_DIR)/$(bfs_crate).elf
$(bfs): $(bfs).intermediate

.INTERMDIATE: $(bfs).intermediate
$(bfs).intermediate: $(init) $(eth_driver) $(eth_virt_rx) $(eth_virt_tx) $(echo_server) $(eth_copier) $(timer)
	SEL4_PREFIX=$(sel4_prefix) \
	INIT_ELF=$(init) \
	ETH_DRIVER_ELF=$(eth_driver) \
	ETH_VIRT_RX_ELF=$(eth_virt_rx) \
	ETH_VIRT_TX_ELF=$(eth_virt_tx) \
	ETH_COPIER_ELF=$(eth_copier) \
	ECHO_SERVER_ELF=$(echo_server) \
	TIMER_ELF=$(timer) \
		cargo build \
			-Z build-std=core,alloc,compiler_builtins \
			-Z build-std-features=compiler-builtins-mem \
			-Z unstable-options \
			--target support/targets/aarch64-sel4.json \
			--target-dir $(abspath $(BUILD_DIR)/target) \
			--out-dir $(BUILD_DIR) \
			-p $(bfs_crate)

# Build the loader
smos_loader_crate := smos-loader
smos_loader := $(BUILD_DIR)/$(smos_loader_crate).elf
$(smos_loader): $(smos_loader).intermediate

.INTERMDIATE: $(smos_loader).intermediate
$(smos_loader).intermediate:
	SEL4_PREFIX=$(sel4_prefix) \
	LOADER_LINKER_SCRIPT=$(shell pwd)/crates/smos-loader/custom.ld \
		cargo build \
			-Z build-std=core,alloc,compiler_builtins \
			-Z build-std-features=compiler-builtins-mem \
			-Z unstable-options \
			--target support/targets/aarch64-sel4.json \
			--target-dir $(abspath $(BUILD_DIR)/target) \
			--out-dir $(BUILD_DIR) \
			-p $(smos_loader_crate)

# Build the root server
root_server_crate := root_server
root_server := $(BUILD_DIR)/$(root_server_crate).elf
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
			--target-dir $(abspath $(BUILD_DIR)/target) \
			--out-dir $(BUILD_DIR) \
			-p $(root_server_crate)


image := $(BUILD_DIR)/image.elf
# Append the payload to the loader using the loader CLI
#$(image): $(root_server)
build/image.elf: $(root_server)
	$(loader_cli) \
		--loader $(loader) \
		--sel4-prefix $(sel4_prefix) \
		--app $(root_server) \
		-o $@

qemu_cmd := \
	qemu-system-aarch64 \
		-machine virt,virtualization=on \
		-cpu cortex-a57 -m size=2G \
		-serial mon:stdio \
		-device virtio-net-device,netdev=netdev0 \
		-netdev user,id=netdev0,hostfwd=tcp::1236-:1236,hostfwd=udp::1235-:1235 \
		-global virtio-mmio.force-legacy=false \
		-nographic \
		-kernel $(image)

# 		-s -S \
#dumpdtb=qemu.dtb \

.PHONY: run
run: $(image)
	$(qemu_cmd)

.PHONY: test
test: test.py $(image)
	python3 $< $(qemu_cmd)
