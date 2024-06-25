#
# Copyright 2023, Colias Group, LLC
#
# SPDX-License-Identifier: BSD-2-Clause
#

# Basis for seL4 kernel configuration

set(ARM_CPU cortex-a57 CACHE STRING "")
#set(ARM_CPU cortex-a53 CACHE STRING "")
set(KernelIsMCS ON CACHE BOOL "" FORCE)
set(KernelArch arm CACHE STRING "")
set(KernelArmHypervisorSupport ON CACHE BOOL "")
set(KernelMaxNumNodes 1 CACHE STRING "")
set(KernelPlatform qemu-arm-virt CACHE STRING "")
#set(KernelPlatform odroidc2 CACHE STRING "")
set(KernelSel4Arch aarch64 CACHE STRING "")
set(KernelVerificationBuild OFF CACHE BOOL "")
set(KernelArmExportPCNTUser ON CACHE BOOL "")
set(KernelArmExportPTMRUser ON CACHE BOOL "")
set(KernelDebugBuild ON CACHE BOOL "")
set(KernelPrinting ON CACHE BOOL "")
set(KernelRootCNodeSizeBits 13 CACHE STRING "")
