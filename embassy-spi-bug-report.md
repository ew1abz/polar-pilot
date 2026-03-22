# STM32L4: SPI CLK goes hi-Z between DMA operations in `SpiDevice::transaction()`, corrupting multi-operation SPI frames

## Description

When using `embedded-hal-async` `SpiDevice::transaction()` with multiple `Operation`s on STM32L432KC, the SPI clock (SCK) pin goes hi-Z between DMA transfers. This corrupts communication with slave devices (e.g., W5500 Ethernet controller) that expect continuous clocking within a single CS-asserted transaction.

The `embassy-net-wiznet` driver splits W5500 SPI frames into separate operations:

```rust
let operations = &mut [
    Operation::Write(&header),        // 3-byte address+control
    Operation::TransferInPlace(data), // N data bytes
];
spi.transaction(operations).await
```

Between these operations, the SPI peripheral is disabled (`SPE=0`), which releases SCK to hi-Z on STM32L4. The W5500 loses frame sync, causing corrupted register reads and broken networking.

## Root Cause

The `embassy-stm32` SPI driver's `transfer_inner()` disables the SPI peripheral (`SPE=0`) between DMA operations. On STM32L4, this releases all SPI pins to their default GPIO state (hi-Z). The `SpiDevice::transaction()` implementation loops over operations calling bus methods sequentially, each of which independently enables/disables the peripheral.

The `embedded-hal-async` `SpiDevice::transaction()` contract guarantees CS stays asserted across operations, but makes no guarantee about CLK stability between them. However, CLK floating hi-Z mid-transaction is a protocol violation that affects any slave device sensitive to clock edges.

## Steps to Reproduce

1. STM32L432KC + W5500 on SPI1 (any SPI bus likely affected)
2. Use `embassy-net-wiznet` 0.3.0 (or any driver calling `transaction()` with multiple operations)
3. Monitor SCK with an oscilloscope during a register read
4. Observe: SCK goes hi-Z between the header write and data transfer

## Expected Behavior

SCK should remain actively driven (idle low for CPOL=0) throughout the entire transaction while CS is asserted.

## Actual Behavior

SCK floats hi-Z between operations. Consequences:
- W5500 register reads return corrupted data
- `PHYCFGR` reads wrong, `is_link_up()` always returns false
- embassy-net drops all received frames, DHCP never completes
- Integer underflow panic in `read_frame()` from corrupted frame size headers

## Platform

- **MCU**: STM32L432KC (Cortex-M4F)
- **embassy-stm32**: 0.6.0
- **embassy-net-wiznet**: 0.3.0
- **embedded-hal-async**: 1.0.0

Likely affects other STM32 families where `SPE=0` releases pins to hi-Z.

## Workarounds

1. **Hardware pull-down on SCK** — keeps SCK low during hi-Z gaps. Works but shouldn't be necessary.

2. **Single-buffer transfers** — combine header + data into one contiguous buffer and use a single `transfer_in_place`. Eliminates the operation boundary entirely.

## Suggested Fix

The SPI driver should not disable the peripheral (`SPE=0`) between operations within a `transaction()`. Either:

1. Keep `SPE=1` between consecutive DMA operations when the bus is in a transaction context
2. Configure SCK GPIO to hold its idle level when SPE is cleared (push-pull output at idle level rather than hi-Z)
3. Provide a transaction-aware DMA path that chains operations without disabling the peripheral

## Impact

Any multi-operation `SpiDevice::transaction()` on STM32L4 is affected. This breaks the W5500 driver and likely any other SPI slave that is sensitive to clock state during a transaction.
