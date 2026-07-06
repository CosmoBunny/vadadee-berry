//! Minimal SVG `d` parser (M, L, C, Z) for MCP path creation.

use kurbo::{BezPath, PathEl};

pub fn bez_from_svg_d(d: &str) -> Result<BezPath, String> {
    let tokens = tokenize(d);
    let mut bez = BezPath::new();
    let mut i = 0usize;
    let mut cx = 0.0_f64;
    let mut cy = 0.0_f64;
    let mut sx = 0.0_f64;
    let mut sy = 0.0_f64;
    let mut cmd = b'M';

    fn next_num(tokens: &[String], i: &mut usize) -> Result<f64, String> {
        let t = tokens.get(*i).ok_or("unexpected end of path data")?;
        *i += 1;
        t.parse::<f64>().map_err(|_| format!("invalid number: {t}"))
    }

    while i < tokens.len() {
        let t = &tokens[i];
        if t.len() == 1 && t.chars().next().unwrap().is_ascii_alphabetic() {
            cmd = t.as_bytes()[0];
            i += 1;
            if cmd == b'Z' || cmd == b'z' {
                bez.close_path();
                cx = sx;
                cy = sy;
            }
            continue;
        }
        match cmd {
            b'M' => {
                let x = next_num(&tokens, &mut i)?;
                let y = next_num(&tokens, &mut i)?;
                bez.move_to((x, y));
                cx = x;
                cy = y;
                sx = x;
                sy = y;
                cmd = b'L';
            }
            b'm' => {
                let x = cx + next_num(&tokens, &mut i)?;
                let y = cy + next_num(&tokens, &mut i)?;
                bez.move_to((x, y));
                cx = x;
                cy = y;
                sx = x;
                sy = y;
                cmd = b'l';
            }
            b'L' => {
                let x = next_num(&tokens, &mut i)?;
                let y = next_num(&tokens, &mut i)?;
                bez.line_to((x, y));
                cx = x;
                cy = y;
            }
            b'l' => {
                let x = cx + next_num(&tokens, &mut i)?;
                let y = cy + next_num(&tokens, &mut i)?;
                bez.line_to((x, y));
                cx = x;
                cy = y;
            }
            b'C' => {
                let x1 = next_num(&tokens, &mut i)?;
                let y1 = next_num(&tokens, &mut i)?;
                let x2 = next_num(&tokens, &mut i)?;
                let y2 = next_num(&tokens, &mut i)?;
                let x = next_num(&tokens, &mut i)?;
                let y = next_num(&tokens, &mut i)?;
                bez.curve_to((x1, y1), (x2, y2), (x, y));
                cx = x;
                cy = y;
            }
            b'c' => {
                let x1 = cx + next_num(&tokens, &mut i)?;
                let y1 = cy + next_num(&tokens, &mut i)?;
                let x2 = cx + next_num(&tokens, &mut i)?;
                let y2 = cy + next_num(&tokens, &mut i)?;
                let x = cx + next_num(&tokens, &mut i)?;
                let y = cy + next_num(&tokens, &mut i)?;
                bez.curve_to((x1, y1), (x2, y2), (x, y));
                cx = x;
                cy = y;
            }
            _ => return Err(format!("unsupported path command: {}", t)),
        }
    }
    if bez.elements().is_empty() {
        return Err("empty path".into());
    }
    if bez.elements().last() == Some(&PathEl::ClosePath) {
        // ok
    }
    Ok(bez)
}

fn tokenize(d: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<String>| {
        if !cur.is_empty() {
            out.push(cur.clone());
            cur.clear();
        }
    };
    for ch in d.chars() {
        if ch.is_ascii_alphabetic() {
            flush(&mut cur, &mut out);
            out.push(ch.to_string());
        } else if ch == ',' || ch.is_whitespace() {
            flush(&mut cur, &mut out);
        } else {
            cur.push(ch);
        }
    }
    flush(&mut cur, &mut out);
    out
}
