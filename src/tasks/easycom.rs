use core::fmt::Write as FmtWrite;

use defmt::*;
use embassy_stm32::usart;

use crate::types::{RotatorCmd, SoftLimits, CMD, LIMITS, STATE};
use crate::util::{extract_value_after, line_contains, parse_f32_bytes};

#[embassy_executor::task]
pub async fn easycom_task(
    mut usart_rx: usart::UartRx<'static, embassy_stm32::mode::Async>,
    mut usart_tx: usart::UartTx<'static, embassy_stm32::mode::Async>,
) -> ! {
    info!("EasyComm II task started");

    let mut buf = [0u8; 128];
    let mut pos: usize = 0;

    loop {
        let mut tmp = [0u8; 64];
        let n = match usart_rx.read_until_idle(&mut tmp).await {
            Ok(n) => n,
            Err(_) => {
                warn!("USART read error");
                pos = 0;
                continue;
            }
        };

        for i in 0..n {
            let b = tmp[i];

            if b == b'\n' || b == b'\r' {
                if pos == 0 {
                    continue;
                }

                let line = &buf[..pos];
                pos = 0;

                if line == b"VE" {
                    let _ = usart_tx.write(b"Polar Pilot v0.1\n").await;
                } else if line == b"SA" || line == b"SE" || line == b"SA SE" {
                    CMD.send(RotatorCmd::Stop).await;
                } else if line == b"LM" {
                    // Get soft limits
                    let lim = LIMITS.lock(|c| c.get());
                    let mut resp = heapless::String::<64>::new();
                    let _ = core::write!(resp, "LM {:.1} {:.1} {:.1} {:.1}\n",
                        lim.az_min, lim.az_max, lim.el_min, lim.el_max);
                    let _ = usart_tx.write(resp.as_bytes()).await;
                } else if line.starts_with(b"LM ") {
                    // Set soft limits: "LM az_min az_max el_min el_max"
                    let args = &line[3..];
                    let mut parts = args.splitn(4, |&b| b == b' ');
                    let az_min = parts.next().and_then(parse_f32_bytes);
                    let az_max = parts.next().and_then(parse_f32_bytes);
                    let el_min = parts.next().and_then(parse_f32_bytes);
                    let el_max = parts.next().and_then(parse_f32_bytes);
                    match (az_min, az_max, el_min, el_max) {
                        (Some(az_min), Some(az_max), Some(el_min), Some(el_max)) => {
                            LIMITS.lock(|c| c.set(SoftLimits { az_min, az_max, el_min, el_max }));
                        }
                        _ => { warn!("EasyComm: bad LM args"); }
                    }
                } else if line == b"?" {
                    let _ = usart_tx.write(b"\r").await;
                } else if line == b"C" {
                    let state = STATE.try_get().unwrap_or_default();
                    let mut resp = heapless::String::<40>::new();
                    let _ = core::write!(resp, "AZ{:.1} EL{:.1}\n",
                        state.current_az, state.current_el);
                    let _ = usart_tx.write(resp.as_bytes()).await;
                } else if line.len() > 1 && line[0] == b'A' && line[1] != b'Z' {
                    // GS-232: A<NNN> — set azimuth only
                    if let Some(az) = parse_f32_bytes(&line[1..]) {
                        let state = STATE.try_get().unwrap_or_default();
                        CMD.send(RotatorCmd::GoTo { az, el: state.target_el }).await;
                    } else {
                        warn!("EasyComm: bad A arg");
                    }
                } else if line.len() > 1 && line[0] == b'E' && line[1] != b'L' {
                    // GS-232: E<NNN> — set elevation only
                    if let Some(el) = parse_f32_bytes(&line[1..]) {
                        let state = STATE.try_get().unwrap_or_default();
                        CMD.send(RotatorCmd::GoTo { az: state.target_az, el }).await;
                    } else {
                        warn!("EasyComm: bad E arg");
                    }
                } else if line.starts_with(b"W ") {
                    // GS-232B: W<az> <el> — set both axes
                    let args = &line[2..];
                    let sp = args.iter().position(|&b| b == b' ');
                    if let Some(sp) = sp {
                        match (parse_f32_bytes(&args[..sp]), parse_f32_bytes(&args[sp + 1..])) {
                            (Some(az), Some(el)) => CMD.send(RotatorCmd::GoTo { az, el }).await,
                            _ => warn!("EasyComm: bad W args"),
                        }
                    } else {
                        warn!("EasyComm: bad W format");
                    }
                } else {
                    let has_az = line.starts_with(b"AZ");
                    let has_el = line_contains(line, b"EL");

                    if has_az || has_el {
                        let az_val = extract_value_after(line, b"AZ");
                        let el_val = extract_value_after(line, b"EL");

                        if az_val.is_none() && el_val.is_none() && has_az {
                            let state = STATE.try_get().unwrap_or_default();
                            let mut resp = heapless::String::<40>::new();
                            let _ = core::write!(
                                resp, "AZ{:.1} EL{:.1}\n",
                                state.current_az, state.current_el,
                            );
                            let _ = usart_tx.write(resp.as_bytes()).await;
                        } else {
                            let state = STATE.try_get().unwrap_or_default();
                            let az = az_val.unwrap_or(state.target_az);
                            let el = el_val.unwrap_or(state.target_el);
                            CMD.send(RotatorCmd::GoTo { az, el }).await;
                        }
                    } else {
                        warn!("EasyComm: unknown cmd");
                    }
                }
            } else if pos < buf.len() {
                buf[pos] = b;
                pos += 1;
            } else {
                warn!("EasyComm: line overflow");
                pos = 0;
            }
        }
    }
}
