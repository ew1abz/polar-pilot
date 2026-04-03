# EEPROM Emulation — Persistent Configuration Storage

## Status: Potential Feature

## Motivation

The rotator currently loses all configuration on power cycle. Persisting the
following would improve usability:

- Park position (AZ/EL degrees)
- Soft limits (min/max AZ, min/max EL)
- Network config override (static IP, port)
- Steps-per-degree calibration for each axis
- Last known position (for resume without homing)

## Hardware Constraints

The STM32L432KC has **no dedicated data EEPROM**. The L432 only has 256 KB of
main flash, organized as 2 KB pages. Flash endurance is ~10,000 erase cycles
per page (datasheet §3.10).

Embassy-stm32's `Flash` driver implements the `embedded-storage` `NorFlash`
and `ReadNorFlash` traits, which provides the foundation for emulation.

## Solution: `sequential-storage` on Main Flash

[`sequential-storage`](https://crates.io/crates/sequential-storage) is a
`no_std`, heap-free crate that provides a wear-leveled key-value map on top of
any `NorFlash` implementation. It distributes writes across a configurable
range of flash pages so no single page absorbs all the erase cycles.

### Flash Layout

Allocate the last 4 pages (8 KB) of flash for storage, keeping them out of
the linker's program region:

```text
0x0800_0000  ┌──────────────────────┐
             │  Firmware (≤248 KB)  │
0x0803_E000  ├──────────────────────┤  ← storage start
             │  Page 62  (2 KB)     │
             │  Page 63  (2 KB)     │
             │  Page 64  (2 KB)     │
             │  Page 65  (2 KB)     │
0x0804_0000  └──────────────────────┘  ← end of flash
```

Reserve those pages in `memory.x` so the linker never places code there:

```text
/* memory.x */
MEMORY
{
  FLASH : ORIGIN = 0x08000000, LENGTH = 248K   /* was 256K */
  STORAGE : ORIGIN = 0x0803E000, LENGTH = 8K
  RAM   : ORIGIN = 0x20000000, LENGTH = 48K
}
```

### Dependencies

```toml
# Cargo.toml
[dependencies]
sequential-storage = { version = "4", default-features = false }
embedded-storage = "0.3"
```

### Key Definitions

Define all storable keys as a compact enum. Each variant serializes to a
single `u8` discriminant — the key used in the flash map.

```rust
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CfgKey {
    ParkAz      = 0,
    ParkEl      = 1,
    MinAz       = 2,
    MaxAz       = 3,
    MinEl       = 4,
    MaxEl       = 5,
    StepsPerDegAz = 6,
    StepsPerDegEl = 7,
    LastAz      = 8,
    LastEl      = 9,
}
```

Values are stored as raw `f32` bytes (4 bytes each).

### Storage Helper

```rust
use embassy_stm32::flash::{Flash, WRITE_SIZE};
use sequential_storage::map;

const STORAGE_RANGE: core::ops::Range<u32> = 0x0803_E000..0x0804_0000;

/// Read one f32 config value; returns `default` if not yet written.
pub async fn cfg_read(flash: &mut Flash<'_>, key: CfgKey, default: f32) -> f32 {
    let mut buf = [0u8; 4];
    match map::fetch_item::<_, _, _>(
        flash,
        STORAGE_RANGE.clone(),
        &mut [0u8; 64],   // scratch buffer
        key as u8,
        &mut buf,
    )
    .await
    {
        Ok(Some(bytes)) => f32::from_le_bytes(bytes.try_into().unwrap_or([0u8; 4])),
        _ => default,
    }
}

/// Write one f32 config value.
pub async fn cfg_write(flash: &mut Flash<'_>, key: CfgKey, value: f32) {
    let bytes = value.to_le_bytes();
    map::store_item::<_, _, _>(
        flash,
        STORAGE_RANGE.clone(),
        &mut [0u8; 64],   // scratch buffer
        key as u8,
        &bytes,
    )
    .await
    .ok();  // best-effort; log via defmt in production
}
```

### Integration with motor_task

Load calibration at startup before spawning the motor task:

```rust
// In main(), after Flash is initialized:
let mut flash = Flash::new(p.FLASH, Irqs);

let steps_az = cfg_read(&mut flash, CfgKey::StepsPerDegAz, 400.0).await;
let steps_el = cfg_read(&mut flash, CfgKey::StepsPerDegEl, 400.0).await;
let park_az  = cfg_read(&mut flash, CfgKey::ParkAz, 0.0).await;
let park_el  = cfg_read(&mut flash, CfgKey::ParkEl, 0.0).await;

spawner.spawn(motor_task(/* ... */ steps_az, steps_el, park_az, park_el)).unwrap();
```

Save position periodically from motor_task (e.g., after each completed slew):

```rust
// Inside motor_task, after slew completes:
cfg_write(&mut flash, CfgKey::LastAz, state.az).await;
cfg_write(&mut flash, CfgKey::LastEl, state.el).await;
```

### Sharing Flash Between Tasks

`embassy-stm32`'s Flash is not `Send`-shareable by default. Options:

1. **Single owner** — pass flash into a dedicated `config_task` and send
   read/write requests via a small channel. Simplest and safest.
2. **Mutex** — wrap in `embassy_sync::mutex::Mutex<CriticalSectionRawMutex, Flash>`,
   share via static. Works but flash ops block interrupt-driven tasks briefly.

Option 1 is recommended for this project since config changes are infrequent.

## Wear Budget

With 4 pages and `sequential-storage`'s wear leveling, each logical key-value
write consumes roughly `1 / (page_count × items_per_page)` of the total erase
budget. At 10,000 cycles × 4 pages and ~10 bytes per entry:

- ~3,200 items per erase cycle sweep
- A write every 10 seconds → **>10 years** before page exhaustion

Writing `LastAz`/`LastEl` after every 100 ms position tick would exhaust flash
in ~1 year — write only on slew completion or on idle timeout.

## Open Questions

- Should static IP config be in flash or hardcoded / provisioned via serial?
- Is resume-without-homing safe? (Position could be stale after manual
  movement with power off.)
- The `Flash` peripheral conflicts with in-application firmware update (IAP)
  — if OTA is added later, the storage region layout must be coordinated.
