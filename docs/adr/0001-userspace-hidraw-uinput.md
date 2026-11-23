# 0001 — Userspace driver instead of a kernel driver

## Context

The CTL-4100 is already handled by the Linux kernel `wacom` driver, but we
want per-application profiles, per-monitor mapping, ExpressKey remapping and a
GUI — things the kernel driver does not expose. Writing a kernel module turns
any bug into a blue-screen risk and makes development harder.

## Decision

Read hidraw and create virtual devices through `/dev/uinput`. The kernel
`wacom` driver stays bound (on this kernel the hidraw node disappears when the
HID driver is unbound). The kernel also owns initialization: resending
`02 02`/`0D 00` over hidraw can pause the stream, so the daemon is passive. It
issues `EVIOCGRAB` on the kernel evdev nodes so the pen is not delivered
twice.

## Consequences

- The daemon runs as a user (group `input`), without root.
- We must reimplement init, parsing and mapping — and take on the maintenance
  cost.
- Other Wacom tablets keep using the kernel driver.
- Without `EVIOCGRAB` there are duplicate events; the grab is released when
  the daemon stops, returning control to the kernel driver.
- The grab requires read access to `/dev/input/event*`; the udev rule uses
  `uaccess` for that.
