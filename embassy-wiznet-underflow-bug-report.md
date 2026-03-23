# embassy-net-wiznet: Integer underflow panic in `read_frame()` on corrupted frame headers

## Description

`WiznetDevice::read_frame()` in `embassy-net-wiznet` 0.3.0 panics with an integer underflow when the W5500 returns a frame size header < 2. The code subtracts 2 from the raw header value without bounds checking:

```rust
// device.rs, read_frame()
let raw = u16::from_be_bytes(frame_bytes) as usize;
raw - 2  // panics when raw < 2
```

This causes a panic: `range end index 4294967294 out of range for slice of length 1514` (0usize - 2 wraps to `usize::MAX - 1`).

## Root Cause

The W5500 MACRAW mode prepends a 2-byte size header to each received frame. The driver subtracts 2 to get the actual frame length. However, if SPI corruption causes the header to read as 0 or 1, the subtraction underflows.

This is triggered in practice by the separate SPI CLK hi-Z bug (see companion report), but any SPI noise or corruption could cause it.

## Steps to Reproduce

1. STM32L432KC + W5500 on SPI1 at 40 MHz
2. Use `embassy-net-wiznet` 0.3.0 with the default multi-operation SPI transactions
3. Receive any Ethernet frame (e.g., DHCP offer)
4. SPI corruption causes the 2-byte frame header to read as 0
5. Panic on `raw - 2` underflow

Observed panic:
```
panicked at 'range end index 4294967294 out of range for slice of length 1514'
embassy-net-wiznet-0.3.0/src/device.rs:216
```

## Fix

Add a bounds check before the subtraction and discard bogus frames:

```rust
let raw = u16::from_be_bytes(frame_bytes) as usize;
if raw < 2 {
    // Corrupted header — advance read pointer and discard
    self.set_rx_read_ptr(read_ptr).await?;
    self.command(Command::Receive).await?;
    return Ok(0);
}
let expected_frame_size = raw - 2;
```

Additionally, cap the read length to the frame buffer size to prevent out-of-bounds access when the header reports a size larger than the buffer:

```rust
let read_len = expected_frame_size.min(frame.len());
```

## Real-World Trigger

At 40 MHz SPI on STM32L4, the CLK hi-Z gap between DMA operations (separate bug) corrupts frame headers consistently, producing bogus frame size 0 on every RX. The bounds check converts this from a hard panic into a graceful discard, keeping the driver running.

## Platform

- **embassy-net-wiznet**: 0.3.0
- **Triggered on**: STM32L432KC with W5500 at 40 MHz SPI
- **Also possible on**: any platform with SPI noise/corruption

## Impact

Hard panic on any corrupted frame header — the device crashes and requires a reset. This is a robustness issue independent of the SPI CLK bug; any transient SPI error could trigger it.
