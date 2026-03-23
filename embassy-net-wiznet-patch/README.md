# WIZnet `embassy-net` integration (patched)

Local patch of [`embassy-net-wiznet`](https://crates.io/crates/embassy-net-wiznet) v0.3.0.

## Changes from upstream

- **Fix integer underflow in `read_frame()`**: Guard against frame size < 2 to prevent panic.
- **Remove redundant software reset**: HW reset pin already resets the chip; SW reset clears PHYCFGR[7] breaking link detection.
- **Combine SPI header into single operation**: Reduces CLK hi-Z gaps between DMA operations on STM32L4.
- **Remove debug logging**: Cleaned up diagnostic messages added during debugging.
