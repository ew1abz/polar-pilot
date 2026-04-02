use core::fmt::Write as FmtWrite;

use defmt::*;
use embassy_net::tcp::TcpSocket;
use embassy_time::{Duration, Timer};
use embedded_io_async::Write as AsyncWrite;

use crate::types::{Phase, RotatorCmd, CMD, LIMITS, STATE};
use crate::util::parse_f32;

#[embassy_executor::task]
pub async fn rotctld_task(stack: embassy_net::Stack<'static>) -> ! {
    let mut rx_buf = [0u8; 256];
    let mut tx_buf = [0u8; 256];

    loop {
        let mut socket = TcpSocket::new(stack, &mut rx_buf, &mut tx_buf);
        socket.set_timeout(Some(Duration::from_secs(30)));

        if socket.accept(4533).await.is_err() {
            Timer::after_millis(500).await;
            continue;
        }

        info!("rotctld: client connected");

        let mut line_buf = [0u8; 128];
        let mut line_len: usize = 0;

        'conn: loop {
            let mut byte = [0u8; 1];
            match socket.read(&mut byte).await {
                Ok(0) | Err(_) => break 'conn,
                Ok(_) => {}
            }

            if byte[0] == b'\n' {
                let end = if line_len > 0 && line_buf[line_len - 1] == b'\r' {
                    line_len - 1
                } else {
                    line_len
                };
                let line = core::str::from_utf8(&line_buf[..end]).unwrap_or("");
                let mut resp: heapless::String<256> = heapless::String::new();

                if line == "p" || line == "\\get_pos" {
                    let state = STATE.try_get().unwrap_or_default();
                    if let Phase::Fault(msg) = state.phase {
                        let _ = core::write!(resp, "FAULT: {}\nRPRT -9\n", msg);
                    } else {
                        let _ = core::write!(resp, "{:.1}\n{:.1}\n", state.current_az, state.current_el);
                    }
                } else if line.starts_with("P ") || line.starts_with("\\set_pos ") {
                    let state = STATE.try_get().unwrap_or_default();
                    if let Phase::Fault(msg) = state.phase {
                        let _ = core::write!(resp, "FAULT: {}\nRPRT -9\n", msg);
                    } else {
                        let args = if line.starts_with("P ") { &line[2..] } else { &line[9..] };
                        let mut parts = args.splitn(2, ' ');
                        let az = parts.next().and_then(parse_f32);
                        let el = parts.next().and_then(parse_f32);
                        match (az, el) {
                            (Some(az), Some(el)) => {
                                CMD.send(RotatorCmd::GoTo { az, el }).await;
                                let _ = core::write!(resp, "RPRT 0\n");
                            }
                            _ => {
                                let _ = core::write!(resp, "RPRT -1\n");
                            }
                        }
                    }
                } else if line == "S" || line == "\\stop" {
                    let state = STATE.try_get().unwrap_or_default();
                    if let Phase::Fault(msg) = state.phase {
                        let _ = core::write!(resp, "FAULT: {}\nRPRT -9\n", msg);
                    } else {
                        CMD.send(RotatorCmd::Stop).await;
                        let _ = core::write!(resp, "RPRT 0\n");
                    }
                } else if line == "l" || line == "\\get_limits" {
                    let lim = LIMITS.lock(|c| c.get());
                    let _ = core::write!(resp, "{:.1}\n{:.1}\n{:.1}\n{:.1}\nRPRT 0\n",
                        lim.az_min, lim.az_max, lim.el_min, lim.el_max);
                } else if line.starts_with("L ") || line.starts_with("\\set_limits ") {
                    let args = if line.starts_with("L ") { &line[2..] } else { &line[12..] };
                    let mut parts = args.splitn(4, ' ');
                    let az_min = parts.next().and_then(parse_f32);
                    let az_max = parts.next().and_then(parse_f32);
                    let el_min = parts.next().and_then(parse_f32);
                    let el_max = parts.next().and_then(parse_f32);
                    match (az_min, az_max, el_min, el_max) {
                        (Some(az_min), Some(az_max), Some(el_min), Some(el_max)) => {
                            LIMITS.lock(|c| c.set(crate::types::SoftLimits { az_min, az_max, el_min, el_max }));
                            let _ = core::write!(resp, "RPRT 0\n");
                        }
                        _ => { let _ = core::write!(resp, "RPRT -1\n"); }
                    }
                } else if line.starts_with("M ") || line.starts_with("\\move ") {
                    let args = if line.starts_with("M ") { &line[2..] } else { &line[6..] };
                    let mut parts = args.splitn(2, ' ');
                    let dir = parts.next().and_then(|s| s.parse::<u8>().ok());
                    // speed arg is accepted but ignored — motor runs at fixed rate
                    let _speed = parts.next().and_then(|s| s.parse::<u8>().ok());
                    let state = STATE.try_get().unwrap_or_default();
                    if let Phase::Fault(msg) = state.phase {
                        let _ = core::write!(resp, "FAULT: {}\nRPRT -9\n", msg);
                    } else {
                        match dir {
                            Some(2)  => { CMD.send(RotatorCmd::GoTo { az: state.current_az,  el:  9999.0 }).await; let _ = core::write!(resp, "RPRT 0\n"); }
                            Some(4)  => { CMD.send(RotatorCmd::GoTo { az: state.current_az,  el: -9999.0 }).await; let _ = core::write!(resp, "RPRT 0\n"); }
                            Some(8)  => { CMD.send(RotatorCmd::GoTo { az: -9999.0, el: state.current_el  }).await; let _ = core::write!(resp, "RPRT 0\n"); }
                            Some(16) => { CMD.send(RotatorCmd::GoTo { az:  9999.0, el: state.current_el  }).await; let _ = core::write!(resp, "RPRT 0\n"); }
                            _        => { let _ = core::write!(resp, "RPRT -1\n"); }
                        }
                    }
                } else if line == "1" || line == "\\dump_caps" {
                    let lim = LIMITS.lock(|c| c.get());
                    let _ = core::write!(resp,
                        "Caps dump for model: 2\nModel name:\tPolar Pilot\nMfg name:\tCustom\nBackend version:\t{}\nBackend status:\tAlpha\nRotator type:\tAz-El\nCan set position:\tY\nCan get position:\tY\nCan stop:\tY\nCan reset:\tY\nCan move:\tY\nMin Azimuth:\t{:.2}\nMax Azimuth:\t{:.2}\nMin Elevation:\t{:.2}\nMax Elevation:\t{:.2}\nRPRT 0\n",
                        env!("CARGO_PKG_VERSION"),
                        lim.az_min, lim.az_max, lim.el_min, lim.el_max);
                } else if line.starts_with("R ") || line.starts_with("\\reset ") || line == "R" || line == "\\reset" {
                    let state = STATE.try_get().unwrap_or_default();
                    if let Phase::Fault(msg) = state.phase {
                        let _ = core::write!(resp, "FAULT: {}\nRPRT -9\n", msg);
                    } else {
                        // Any reset type: stop motion and park at 0°/0°
                        CMD.send(RotatorCmd::GoTo { az: 0.0, el: 0.0 }).await;
                        let _ = core::write!(resp, "RPRT 0\n");
                    }
                } else if line == "q" || line == "\\quit" {
                    break 'conn;
                } else if line == "_" || line == "\\get_info" {
                    let _ = core::write!(resp,
                        "Model name:\tPolar Pilot\nModel ID:\t2\nMfg name:\tCustom\nSW version:\t{}\nStatus:\t\tAlpha\nMax AZ:\t\t450\nMax EL:\t\t180\nRPRT 0\n",
                        env!("CARGO_PKG_VERSION"));
                } else if line == "\\dump_state" {
                    let lim = LIMITS.lock(|c| c.get());
                    let _ = core::write!(resp,
                        "0\nrot_model=0\nmin_az={:.1}\nmax_az={:.1}\nmin_el={:.1}\nmax_el={:.1}\n0\n0\n",
                        lim.az_min, lim.az_max, lim.el_min, lim.el_max);
                } else {
                    let _ = core::write!(resp, "RPRT -1\n");
                }

                if !resp.is_empty() {
                    if socket.write_all(resp.as_bytes()).await.is_err() {
                        break 'conn;
                    }
                }
                line_len = 0;
            } else if line_len < line_buf.len() {
                line_buf[line_len] = byte[0];
                line_len += 1;
            } else {
                break 'conn;
            }
        }

        info!("rotctld: client disconnected");
        socket.close();
        Timer::after_millis(100).await;
    }
}
