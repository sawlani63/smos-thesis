project('smos-rs', ['c'])

### SETUP PATHS

sddf = 'sddf'
sel4_install_dir = 'deps/seL4/install'
echo_server_dir = sddf / 'examples/echo_server'
eth_driver_dir = sddf / 'drivers/network/virtio'
serial_driver_dir = sddf / 'drivers/serial/virtio'
net_components_dir = sddf / 'network/components'
lib_sddf_lwip_dir = sddf / 'network/lib_sddf_lwip'
serial_components_dir = sddf / 'serial/components'
timer_dir = sddf / 'drivers/timer/arm'
lwip_dir = sddf / 'network/ipstacks/lwip/src'
lwip_dir_core = lwip_dir / 'core'
lwip_dir_core4 = lwip_dir_core / 'ipv4'


libc_path = run_command('aarch64-none-elf-gcc', '--print-file-name', 'libc.a').stdout().strip()
libc_full_path = run_command('realpath', libc_path).stdout().strip()
libc_dir = run_command('dirname', libc_full_path).stdout().strip()


# SETUP INCLUDE DIRECTORIES

sddf_include_directories = include_directories(sddf / 'include', sddf / 'include/extern', sel4_install_dir / 'libsel4/include')
lwip_include_directories = include_directories(lwip_dir / 'include', echo_server_dir / 'include/lwip' )

# BUILD SDDF C LIBRARIES

# Build lib_sddf_util
libsddf_util = static_library('sddf_util',
                              [sddf / 'util/printf.c',
                               sddf / 'util/assert.c',
                               sddf / 'util/putchar_debug.c',
                               sddf / 'util/cache.c'],
                               include_directories: sddf_include_directories)

libsddf_util_serial = static_library('sddf_util_serial',
                              [sddf / 'util/printf.c',
                               sddf / 'util/assert.c',
                               sddf / 'util/cache.c',
                               sddf / 'util/putchar_serial.c'],
                               include_directories: sddf_include_directories)


# @alwin: Do I need to add ethernet.h etc.
# Build the sDDF ethernet driver
libeth_driver = static_library('eth_driver',
                               eth_driver_dir / 'ethernet.c',
                               include_directories: sddf_include_directories,
                               link_with: libsddf_util)

# Build the sDDF virtio serial driver
libserial_driver = static_library('serial_driver',
                                  serial_driver_dir / 'console.c',
                                  include_directories: [sddf_include_directories, serial_driver_dir / 'include'],
                                  link_with: libsddf_util)

# Build the sDDF network components

# Build the rx_virt
libeth_virt_rx = static_library('eth_virt_rx',
                                net_components_dir / 'virt_rx.c',
                                include_directories: sddf_include_directories,
                                link_with: libsddf_util)

# Build the tx_virt
libeth_virt_tx = static_library('eth_virt_tx',
                                net_components_dir / 'virt_tx.c',
                                include_directories: sddf_include_directories,
                                link_with: libsddf_util)

# Build the copy
libeth_copy = static_library('copy',
                             net_components_dir / 'copy.c',
                             include_directories: sddf_include_directories,
                             link_with: libsddf_util)

# Build the timer
libtimer = static_library('timer',
                          timer_dir / 'timer.c',
                          include_directories: sddf_include_directories,
                          link_with: libsddf_util)


# Build the sDDF serial components

# Build the rx_virt

libserial_virt_rx = static_library('serial_virt_rx',
                                   serial_components_dir / 'virt_rx.c',
                                   include_directories: sddf_include_directories,
                                   link_with: libsddf_util)

# Build the tx_virt

libserial_virt_tx = static_library('serial_virt_tx',
                                   serial_components_dir / 'virt_tx.c',
                                   include_directories: sddf_include_directories,
                                   link_with: libsddf_util)
# Build echo server

# Build newlib
newlib = static_library('newlib',
                        sddf / 'util/newlibc.c',
                        include_directories: sddf_include_directories)

# Add libc as dependency


# cc = meson.get_compiler('c')
# libc = declare_dependency(
#   dependencies: cc.find_library('c', dirs : [libc_dir])
# )

# Build lwip
corefiles = [lwip_dir_core / 'init.c',
             lwip_dir_core / 'def.c',
             lwip_dir_core / 'dns.c',
             lwip_dir_core / 'inet_chksum.c',
             lwip_dir_core / 'ip.c',
             lwip_dir_core / 'mem.c',
             lwip_dir_core / 'memp.c',
             lwip_dir_core / 'netif.c',
             lwip_dir_core / 'pbuf.c',
             lwip_dir_core / 'raw.c',
             lwip_dir_core / 'stats.c',
             lwip_dir_core / 'sys.c',
             lwip_dir_core / 'altcp.c',
             lwip_dir_core / 'altcp_alloc.c',
             lwip_dir_core / 'altcp_tcp.c',
             lwip_dir_core / 'tcp.c',
             lwip_dir_core / 'tcp_in.c',
             lwip_dir_core / 'tcp_out.c',
             lwip_dir_core / 'timeouts.c',
             lwip_dir_core / 'udp.c'
]

core4files = [lwip_dir_core4 / 'autoip.c',
              lwip_dir_core4 / 'dhcp.c',
              lwip_dir_core4 / 'etharp.c',
              lwip_dir_core4 / 'icmp.c',
              lwip_dir_core4 / 'igmp.c',
              lwip_dir_core4 / 'ip4_frag.c',
              lwip_dir_core4 / 'ip4.c',
              lwip_dir_core4 / 'ip4_addr.c']

api_files = lwip_dir / 'api/err.c'
netif_files = lwip_dir / 'netif/ethernet.c'

lib_sddf_lwip = static_library('lib_sddf_lwip',
                               [corefiles, core4files, api_files, netif_files, lib_sddf_lwip_dir / 'lib_sddf_lwip.c'],
                               link_with: [newlib, libsddf_util_serial],
                               include_directories: [sddf_include_directories, lwip_include_directories])

# Build lib echo server
libecho_server = static_library('echo_server',
                                [echo_server_dir / 'echo.c', echo_server_dir / 'udp_echo_socket.c', echo_server_dir / 'tcp_echo_socket.c' ],
                                link_with: [libsddf_util_serial, lib_sddf_lwip],
                                include_directories: [sddf_include_directories, lwip_include_directories]
)

# Define environment variables
cargo_env = environment()
cargo_env.set('CARGO_TERM_COLOR', 'always')
cargo_env.set('SEL4_PREFIX', meson.source_root() / 'deps/seL4/install')
cargo_env.set('LOADER_ELF', meson.current_build_dir() / 'smos-loader.elf')
cargo_env.set('BOOT_FS_ELF', meson.current_build_dir() / 'boot_file_server.elf')
cargo_env.set('ETH_DRIVER_ELF', meson.current_build_dir() / 'eth_driver.elf')
cargo_env.set('ETH_VIRT_RX_ELF', meson.current_build_dir() / 'eth_virt_rx.elf')
cargo_env.set('ETH_VIRT_TX_ELF', meson.current_build_dir() / 'eth_virt_tx.elf')
cargo_env.set('ETH_COPIER_ELF', meson.current_build_dir() / 'eth_copier.elf')
cargo_env.set('ECHO_SERVER_ELF', meson.current_build_dir() / 'echo_server.elf')
cargo_env.set('INIT_ELF', meson.current_build_dir() / 'init.elf')
cargo_env.set('TIMER_ELF', meson.current_build_dir() / 'timer.elf')
cargo_env.set('SERIAL_DRIVER_ELF', meson.current_build_dir() / 'virtio_serial_driver.elf')
cargo_env.set('SERIAL_VIRT_RX_ELF', meson.current_build_dir() / 'serial_virt_rx.elf')
cargo_env.set('SERIAL_VIRT_TX_ELF', meson.current_build_dir() / 'serial_virt_tx.elf')


cargo_env.set('LOADER_LINKER_SCRIPT', meson.source_root() / 'crates/smos-loader/custom.ld')
cargo_env.set('BUILD_DIR', meson.current_build_dir())
cargo_env.set('LIBC_DIR', libc_dir)

cargo_command_base = [
    'cargo', 'build', '--verbose',
    '-Z', 'build-std=core,alloc,compiler_builtins',
    '-Z', 'build-std-features=compiler-builtins-mem',
    '-Z', 'unstable-options',
    '--target', meson.source_root() / 'support/targets/aarch64-sel4.json',
    '--target-dir', meson.current_build_dir() / 'target',
    '--out-dir', meson.current_build_dir()
]

# @alwin: There is a slight problem with having the crates built by an external cargo command.
# Meson cannot track dependencies properly, so we must have build_always stale to ensure that
# the cargo targets are built every time. One issue with having this enabled for everything
# is that all of the build execute, and if one fails, its output may be lost in the clutter
# of all the other builds. For this reason, I make each custom target depend on the previous
# one, so that the first one to fail halts the build process. This may slow down the build
# as I think it might reduce parallelism. Still faster than Make tho (and seemingly more reliable)

# Build eth_driver
eth_driver = custom_target(
  'eth_driver',
  output: 'eth_driver.elf',
  command: cargo_command_base + ['-p', 'eth_driver'],
  env: cargo_env,
  build_always_stale: true,
  depends: [libeth_driver]
)

serial_driver = custom_target(
  'virtio_serial_driver',
  output: 'virtio_serial_driver.elf',
  command: cargo_command_base + ['-p', 'virtio_serial_driver'],
  env: cargo_env,
  build_always_stale: true,
  depends: [libserial_driver, eth_driver]
)

# Build the virt_rx
eth_virt_rx = custom_target(
  'eth_virt_rx',
  output: 'eth_virt_rx.elf',
  command: cargo_command_base + ['-p', 'eth_virt_rx'],
  env: cargo_env,
  build_always_stale: true,
  depends: [libeth_virt_rx, serial_driver]
)

# Build the virt_rx
serial_virt_rx = custom_target(
  'serial_virt_rx',
  output: 'serial_virt_rx.elf',
  command: cargo_command_base + ['-p', 'serial_virt_rx'],
  env: cargo_env,
  build_always_stale: true,
  depends: [libserial_virt_rx, eth_virt_rx]
)

# Build the virt_tx
eth_virt_tx = custom_target(
  'eth_virt_tx',
  output: 'eth_virt_tx.elf',
  command: cargo_command_base + ['-p', 'eth_virt_tx'],
  env: cargo_env,
  build_always_stale: true,
  depends: [libeth_virt_tx, serial_virt_rx]
)

# Build the virt_rx
serial_virt_tx = custom_target(
  'serial_virt_tx',
  output: 'serial_virt_tx.elf',
  command: cargo_command_base + ['-p', 'serial_virt_tx'],
  env: cargo_env,
  build_always_stale: true,
  depends: [libserial_virt_rx, eth_virt_tx]
)

# Build the copier
eth_copier = custom_target(
  'eth_copier',
  output: 'eth_copier.elf',
  command: cargo_command_base + ['-p', 'eth_copier'],
  env: cargo_env,
  build_always_stale: true,
  depends: [libeth_copy, serial_virt_tx]
)

# Build the timer
timer = custom_target(
  'timer',
  output: 'timer.elf',
  command: cargo_command_base + ['-p', 'timer'],
  env: cargo_env,
  build_always_stale: true,
  depends: [libtimer, eth_copier]
)

# Built the init task
init = custom_target(
  'init',
  output:  'init.elf',
  command: cargo_command_base + ['-p', 'init'],
  build_always_stale: true,
  depends: [timer],
  env: cargo_env,
)

# Build the echo server
echo_server = custom_target(
  'echo_server',
  output: 'echo_server.elf',
  command:  cargo_command_base + ['-p', 'echo_server'],
  env: cargo_env,
  build_always_stale: true,
  depends: [libecho_server, init]
)

# BUILD BFS
boot_file_server = custom_target(
  'boot_file_server',
  output: 'boot_file_server.elf',
  command: cargo_command_base + ['-p', 'boot_file_server'],
  env: cargo_env,
  build_always_stale: true,
  depends: [init, timer, eth_copier, eth_virt_rx, eth_virt_tx, eth_driver, echo_server]
)

# BUILD LOADER
smos_loader = custom_target(
  'smos-loader',
  output: 'smos-loader.elf',
  command: cargo_command_base + ['-p', 'smos-loader'],
  build_always_stale: true,
  depends: [boot_file_server],
  env: cargo_env
)

# BUILD ROOT_SERVER
root_server = custom_target(
  'root_server',
  output: 'root_server.elf',
  command: cargo_command_base + ['-p', 'root_server'],
  env: cargo_env,
  build_always_stale: true,
  depends: [smos_loader, boot_file_server],
)

# BUILD THE IMAGE
loader_artifcats_dir = meson.source_root() / 'deps/bin'
loader = loader_artifcats_dir / 'sel4-kernel-loader'
loader_cli = loader_artifcats_dir / 'sel4-kernel-loader-add-payload'

image = custom_target(
  'image',
  output: 'image.elf',
  command: [ loader_cli,
             '--loader', loader,
             '--sel4-prefix', meson.source_root() / 'deps/seL4/install',
             '--app', meson.current_build_dir() / 'root_server.elf',
             '-o', meson.current_build_dir() / 'image.elf'],
  depends: [root_server],
  build_by_default: true,
  )

# custom_target('qemu',
#   output: 'run',
#   command: ['qemu-system-aarch64',
#             '-machine', 'virt,virtualization=on',
#             '-cpu', 'cortex-a57',
#             '-m', 'size=2G',
#             '-serial', 'mon:stdio',
#             '-device', 'virtio-net-device,netdev=netdev0',
#             '-netdev', 'user,id=netdev0,hostfwd=tcp::1236-:1236,hostfwd=udp::1235-:1235',
#             '-global', 'virtio-mmio.force-legacy=false',
#             '-nographic',
#             '-kernel', meson.current_build_dir() / 'image.elf'],
#   capture: true,
#   depends: [image],
# )


