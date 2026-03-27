# Hardware Tests

Standalone binaries for testing individual peripherals on the STM32L432KC
Nucleo-32 board. Each test is self-contained — no dependencies between them.

## Prerequisites

```
rustup target add thumbv7em-none-eabihf
cargo install probe-rs-tools
```

Connect the Nucleo board via USB. Output is via RTT (Real-Time Transfer)
shown in the `cargo run` terminal.

## Tests

### button_test — 5-way navigation switch

Polls all 5 buttons at 20 ms and logs press/release events.
LED turns on while any button is held.

**Pins**: UP=PB1, DOWN=PB0, LEFT=PA11, RIGHT=PA12, CENTER=PC15
(active-low, internal pull-up)

```
cargo run --release --bin button_test
```

### motor_test — AZ/EL stepper motors

Asserts motor enable (PB4 low), then drives both steppers via hardware PWM.
Toggles direction on both axes every 2 seconds. LED blinks as heartbeat.

**Pins**: AZ STEP=PA8 (TIM1), EL STEP=PA0 (TIM2), AZ DIR=PC14, EL DIR=PA9,
Motor EN=PB4 (active-low)

```
cargo run --release --bin motor_test
```

### oled_test — SSD1306 128x64 OLED display

Initializes the SSD1306 over I2C1, then draws a title ("Polar Pilot"),
a counter incrementing every second, and a dot cycling through 4 corners.
LED toggles each update.

**Pins**: I2C1 — PB6/SCL, PB7/SDA (address 0x3C)

```
cargo run --release --bin oled_test
```

### w5500_test — W5500 SPI Ethernet

Resets the W5500 via PA1, then reads the VERSIONR register over SPI1.
Expects `0x04`. Logs attempt count and blinks LED fast on success.

**Pins**: SPI1 (PA5/SCK, PA6/MISO, PA7/MOSI, PA4/CS), RST=PA1

```
cargo run --release --bin w5500_test
```

## Adding a new test

Create `src/bin/<name>.rs` with `#![no_std]`, `#![no_main]`, and an
`#[embassy_executor::main]` entry point. It will be picked up automatically
by Cargo as a separate binary target.
