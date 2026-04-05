# Bill of Materials — Polar Pilot

Two-axis antenna rotator controller based on STM32L432KC + W5500 SPI Ethernet.

| # | Component | Part / Description | Qty | Datasheet | Amazon |
|---|-----------|-------------------|-----|-----------|--------|
| 1 | MCU Board | STM32L432KC Nucleo-32 | 1 | [UM1956](https://www.st.com/resource/en/user_manual/um1956-stm32-nucleo32-boards-mb1180-stmicroelectronics.pdf) | [B077GFHLFS](https://www.amazon.com/Nucleo-32-development-STM32L432KC-supports-connectivity/dp/B077GFHLFS) |
| 2 | Ethernet Module | W5500 SPI Ethernet | 1 | [W5500 Datasheet](https://docs.wiznet.io/Product/iEthernet/W5500/datasheet) | [B0CDWX9VQ5](https://www.amazon.com/dp/B0CDWX9VQ5) |
| 3 | Display | SSD1306 128×64 OLED, I2C | 1 | [SSD1306 Datasheet](https://cdn-shop.adafruit.com/datasheets/SSD1306.pdf) | [B09T6SJBV5](https://www.amazon.com/dp/B09T6SJBV5) |
| 4 | Motor Driver | A4988 Stepper Driver (×2) | 2 | [A4988 Datasheet](https://www.allegromicro.com/~/media/Files/Datasheets/A4988-Datasheet.ashx) | [B07BND65C8](https://www.amazon.com/dp/B07BND65C8) |
| 5 | Navigation | 5-way joystick nav button module | 1 | — | [B09DPMQ1F3](https://www.amazon.com/dp/B09DPMQ1F3) |
| 6 | Endstops | Microswitch, SPDT, through-hole (×2) | 2 | [Search Omron SS-5](https://omronfs.omron.com/en_US/ecb/products/pdf/en-ss.pdf) | [B07X142VGC](https://www.amazon.com/dp/B07X142VGC) |
| 7 | Enclosure | Serpac 151 BK | 1 | [Serpac 151](https://www.serpac.com/151.aspx) | [Search](https://www.amazon.com/s?k=Serpac+151+BK) |
| 8 | Prototype PCB | Perforated PCB 7×9 cm | 1 | — | [B07FFDFLZ3](https://www.amazon.com/dp/B07FFDFLZ3) |
| 9 | DC-DC Converter | Buck module (e.g. MP1584, 28 V → 3.3 V) | 1 | — | [B089GV88DK](https://www.amazon.com/dp/B089GV88DK) |
| 10 | Power Connector | Panel-mount barrel jack or terminal block | 1 | — | [B0DLKN8J7M](https://www.amazon.com/dp/B0DLKN8J7M) |
| 11 | PoE Adaptor | PoE splitter | 1 | — | [B01HMNJHII](https://www.amazon.com/dp/B01HMNJHII) |
| 12 | Standoffs | M3 hex standoffs assortment | 1 set | — | [Search](https://www.amazon.com/s?k=M3+hex+standoff+assortment) |

## Notes

- Four 0 Ω resistors must be removed from the Nucleo board (SB1, SB2, SB16, SB18) before assembly. See [SPEC.md](SPEC.md) for details.
