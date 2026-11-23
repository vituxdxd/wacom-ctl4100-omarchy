# 0002 — Use the structured Report `0x10`, not the `0xAC` stream

## Context

The macOS driver reads the 27-byte USB report as 8-byte sub-reports
(`TranslateShortEMR`) and also has a 191-byte vendor stream path `0xAC`. The
HID descriptor, on the other hand, defines clear fields for Report `0x10`:
24-bit X/Y, 16-bit pressure, tip/button flags.

## Decision

Parse Report `0x10` according to the descriptor. `0xAC` and the 8-byte path
stay documented but unimplemented.

## Consequences

- The parser is simple, testable and identical to what the Linux `wacom`
  driver uses.
- We lose any information that exists only in the raw stream (if any).
- If a future firmware only sends `0xAC`, a second parser will be needed.
