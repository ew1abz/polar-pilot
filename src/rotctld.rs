//! Hamlib rotctld protocol handler and helpers.

use defmt::*;
use embassy_net::tcp::TcpSocket;
use embedded_io_async::Write;

/// Rotator state — in-memory stub (no real motors).
pub struct RotatorState {
    pub azimuth: f32,
    pub elevation: f32,
}

pub static mut ROTATOR: RotatorState = RotatorState {
    azimuth: 0.0,
    elevation: 0.0,
};

/// Handle a single TCP connection, processing rotctld commands.
pub async fn handle_connection(socket: &mut TcpSocket<'_>) {
    let mut buf = [0u8; 256];
    let mut line_buf = [0u8; 256];
    let mut line_len: usize = 0;

    loop {
        let n = match socket.read(&mut buf).await {
            Ok(0) => return, // EOF — client disconnected
            Ok(n) => n,
            Err(_) => return,
        };

        // Process received bytes, looking for newline-terminated commands
        for &byte in &buf[..n] {
            if byte == b'\n' {
                let line = &line_buf[..line_len];
                if let Err(_) = process_command(line, socket).await {
                    return; // Write error or quit command
                }
                line_len = 0;
            } else if line_len < line_buf.len() {
                line_buf[line_len] = byte;
                line_len += 1;
            }
        }
    }
}

/// Parse and execute a single rotctld command.
/// Returns Err(()) if the connection should be closed.
async fn process_command(line: &[u8], socket: &mut TcpSocket<'_>) -> Result<(), ()> {
    let line = trim(line);

    match line {
        // ── Get position ────────────────────────────────────────
        b"p" | b"\\get_pos" => {
            let (az, el) = unsafe { (ROTATOR.azimuth, ROTATOR.elevation) };
            let mut resp = [0u8; 64];
            let n = format_position(&mut resp, az, el);
            socket.write_all(&resp[..n]).await.map_err(|_| ())?;
        }

        // ── Stop ────────────────────────────────────────────────
        b"S" | b"\\stop" => {
            info!("Stop command received");
            socket.write_all(b"RPRT 0\n").await.map_err(|_| ())?;
        }

        // ── Get info ────────────────────────────────────────────
        b"_" | b"\\get_info" => {
            socket
                .write_all(b"Model: W5500 Rotator\n")
                .await
                .map_err(|_| ())?;
        }

        // ── Quit ────────────────────────────────────────────────
        b"q" | b"Q" => {
            return Err(()); // Signal to close connection
        }

        // ── Dump state (compatibility) ──────────────────────────
        b"\\dump_state" => {
            socket
                .write_all(
                    b"0\nrot_model=0\nmin_az=0.0\nmax_az=360.0\nmin_el=0.0\nmax_el=90.0\n0\n0\n",
                )
                .await
                .map_err(|_| ())?;
        }

        // ── Set position: "P <az> <el>" ────────────────────────
        _ if line.starts_with(b"P ") || line.starts_with(b"\\set_pos ") => {
            let args = if line.starts_with(b"P ") {
                &line[2..]
            } else {
                &line[9..]
            };
            if let Some((az, el)) = parse_two_floats(args) {
                unsafe {
                    ROTATOR.azimuth = az;
                    ROTATOR.elevation = el;
                }
                info!("Set position: az={}, el={}", az, el);
                socket.write_all(b"RPRT 0\n").await.map_err(|_| ())?;
            } else {
                socket.write_all(b"RPRT -1\n").await.map_err(|_| ())?;
            }
        }

        // ── Unknown command ─────────────────────────────────────
        _ => {
            warn!("Unknown command");
            socket.write_all(b"RPRT -1\n").await.map_err(|_| ())?;
        }
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════
// Helpers — no-std float formatting and parsing
// ═══════════════════════════════════════════════════════════════════

/// Format "az\nel\n" into the buffer.  Returns bytes written.
fn format_position(buf: &mut [u8], az: f32, el: f32) -> usize {
    let mut pos = 0;
    pos += format_f32(&mut buf[pos..], az);
    buf[pos] = b'\n';
    pos += 1;
    pos += format_f32(&mut buf[pos..], el);
    buf[pos] = b'\n';
    pos += 1;
    pos
}

/// Minimal f32 formatter: writes "[-]digits.d" (one decimal place).
fn format_f32(buf: &mut [u8], val: f32) -> usize {
    let mut pos = 0;
    let val = if val < 0.0 {
        buf[pos] = b'-';
        pos += 1;
        -val
    } else {
        val
    };
    let integer = val as u32;
    let frac = ((val - integer as f32) * 10.0) as u8;
    pos += format_u32(&mut buf[pos..], integer);
    buf[pos] = b'.';
    pos += 1;
    buf[pos] = b'0' + frac;
    pos += 1;
    pos
}

/// Format a u32 into decimal ASCII.  Returns bytes written.
fn format_u32(buf: &mut [u8], mut val: u32) -> usize {
    if val == 0 {
        buf[0] = b'0';
        return 1;
    }
    let mut tmp = [0u8; 10];
    let mut len = 0;
    while val > 0 {
        tmp[len] = b'0' + (val % 10) as u8;
        val /= 10;
        len += 1;
    }
    for i in 0..len {
        buf[i] = tmp[len - 1 - i];
    }
    len
}

/// Parse "az el" from a byte slice.
fn parse_two_floats(input: &[u8]) -> Option<(f32, f32)> {
    let input = trim(input);
    let space = input.iter().position(|&b| b == b' ')?;
    let az = parse_f32(&input[..space])?;
    let el = parse_f32(trim(&input[space + 1..]))?;
    Some((az, el))
}

/// Minimal f32 parser for ASCII decimal like "123.4" or "-5.67".
fn parse_f32(s: &[u8]) -> Option<f32> {
    if s.is_empty() {
        return None;
    }
    let (neg, s) = if s[0] == b'-' { (true, &s[1..]) } else { (false, s) };
    let mut integer: u32 = 0;
    let mut frac: u32 = 0;
    let mut frac_digits: u32 = 0;
    let mut in_frac = false;

    for &b in s {
        if b == b'.' {
            in_frac = true;
        } else if b.is_ascii_digit() {
            if in_frac {
                frac = frac * 10 + (b - b'0') as u32;
                frac_digits += 1;
            } else {
                integer = integer * 10 + (b - b'0') as u32;
            }
        } else {
            return None;
        }
    }

    let mut val = integer as f32;
    if frac_digits > 0 {
        let mut divisor = 1u32;
        for _ in 0..frac_digits {
            divisor *= 10;
        }
        val += frac as f32 / divisor as f32;
    }
    if neg {
        val = -val;
    }
    Some(val)
}

/// Trim leading/trailing whitespace and carriage returns.
fn trim(s: &[u8]) -> &[u8] {
    let mut start = 0;
    while start < s.len() && (s[start] == b' ' || s[start] == b'\r' || s[start] == b'\t') {
        start += 1;
    }
    let mut end = s.len();
    while end > start && (s[end - 1] == b' ' || s[end - 1] == b'\r' || s[end - 1] == b'\t') {
        end -= 1;
    }
    &s[start..end]
}
