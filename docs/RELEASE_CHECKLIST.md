# Release Checklist — Polar Pilot

Steps to cut an official release. The GitHub Actions release workflow
(`release.yml`) triggers automatically on a `v*.*.*` tag push and produces
`.bin` and `.hex` artifacts. Everything before that tag is manual.

---

## 1. Code

- [ ] All planned features for this release are merged to `main`
- [ ] No `TODO` / `FIXME` comments left for known blockers
- [ ] `Cargo.toml` version bumped (`version = "x.y.z"`)
- [ ] `Cargo.lock` committed (ensure reproducible builds)

## 2. Protocol Tests (Stub)

Run the full stub suite — no hardware required:

```bash
python3 -m pytest tests/test_protocol.py -v
```

- [ ] All 52 stub tests pass

## 3. Live Hardware Tests

Flash the firmware and run against real hardware:

```bash
cargo run --release

ROTATOR_HOST=<ip> ROTATOR_PORT=/dev/ttyUSB0 \
  python3 -m pytest tests/ -v --timeout=120 --override-ini="addopts="
```

- [ ] DHCP lease acquired (or static fallback at `192.168.1.200`)
- [ ] `p` returns valid AZ/EL from a cold boot
- [ ] `P <az> <el>` drives both axes to target
- [ ] Soft limits enforced (`L` / `LM` set, out-of-range `P` clamped)
- [ ] Two concurrent TCP clients connect simultaneously on port 4533
- [ ] EasyComm II commands work over USART2 (`AZ`, `AZ<n> EL<n>`, `SA`, `SE`, `VE`)
- [ ] GS-232 commands work (`C`, `A<nnn>`, `E<nnn>`, `W <az> <el>`)
- [ ] Homing sequence completes on power-on (both endstops triggered)
- [ ] All live + slow tests pass

## 4. Hardware Test Binaries

Verify each peripheral individually:

```bash
cargo run --release --bin w5500_test
cargo run --release --bin oled_test
cargo run --release --bin motor_test
cargo run --release --bin endstop_test
cargo run --release --bin button_test
```

- [ ] W5500 chip version register reads correctly
- [ ] OLED polar chart renders without artifacts
- [ ] Stepper motors sweep AZ and EL
- [ ] Both endstops register active-low transitions
- [ ] All five nav buttons register correctly

## 5. Documentation

- [ ] `docs/SPEC.md` reflects current pin assignments and hardware table
- [ ] `README.md` features list matches implemented functionality
- [ ] `docs/BOM.md` part list and links are current

## 6. Tag and Push

```bash
git tag v<x.y.z>
git push origin main --tags
```

- [ ] Tag pushed — GitHub Actions `release.yml` starts automatically
- [ ] CI build passes (check Actions tab)
- [ ] Release page shows `.bin` and `.hex` artifacts attached
- [ ] Auto-generated release notes look reasonable; edit if needed
