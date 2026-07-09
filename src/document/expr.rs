//! Tiny math expression evaluator for stack animation functions.
//! Time is **relative to the stack span** (not the global timeline):
//! - `t` = 0 at stack start → 1 at stack end
//! - `f` = 0 at stack start → duration at stack end
//! Also: `s`/`x`/`y`/`r`/`g`/`b`/`a` start constants; + - * / % ^; sin cos tan abs sqrt floor ceil min max pow mod.

#[derive(Debug, Clone)]
pub struct ExprError(pub String);

impl std::fmt::Display for ExprError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Constants available in stack formulas (`x`/`y` position starts, `r`/`g`/`b`/`a` color starts,
/// `s` = this channel's start).
#[derive(Debug, Clone, Copy)]
pub struct ExprVars {
    pub t: f64,
    pub f: f64,
    pub s: f64,
    pub x: f64,
    pub y: f64,
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub a: f64,
}

impl ExprVars {
    pub fn simple(t: f64, f: f64, start: f64) -> Self {
        Self {
            t,
            f,
            s: start,
            x: start,
            y: start,
            r: start,
            g: start,
            b: start,
            a: start,
        }
    }
}

/// Evaluate `expr` with `t` in \[0,1\] and `f` = local frame index from span start.
pub fn eval_expr(expr: &str, t: f64, f: f64) -> Result<f64, ExprError> {
    eval_expr_vars(expr, ExprVars::simple(t, f, 0.0))
}

pub fn eval_expr_vars(expr: &str, vars: ExprVars) -> Result<f64, ExprError> {
    let s = expr.trim();
    if s.is_empty() {
        return Err(ExprError("empty expression".into()));
    }
    let bytes = s.as_bytes();
    let mut i = 0usize;
    let v = parse_expr(bytes, &mut i, vars)?;
    skip_ws(bytes, &mut i);
    if i < bytes.len() {
        return Err(ExprError(format!(
            "unexpected trailing input: {}",
            &s[i..]
        )));
    }
    if !v.is_finite() {
        return Err(ExprError("result is not finite".into()));
    }
    Ok(v)
}

fn skip_ws(b: &[u8], i: &mut usize) {
    while *i < b.len() && b[*i].is_ascii_whitespace() {
        *i += 1;
    }
}

fn parse_expr(b: &[u8], i: &mut usize, vars: ExprVars) -> Result<f64, ExprError> {
    parse_add(b, i, vars)
}

fn parse_add(b: &[u8], i: &mut usize, vars: ExprVars) -> Result<f64, ExprError> {
    let mut v = parse_mul(b, i, vars)?;
    loop {
        skip_ws(b, i);
        match b.get(*i).copied() {
            Some(b'+') => {
                *i += 1;
                v += parse_mul(b, i, vars)?;
            }
            Some(b'-') => {
                *i += 1;
                v -= parse_mul(b, i, vars)?;
            }
            _ => break,
        }
    }
    Ok(v)
}

fn parse_mul(b: &[u8], i: &mut usize, vars: ExprVars) -> Result<f64, ExprError> {
    let mut v = parse_pow(b, i, vars)?;
    loop {
        skip_ws(b, i);
        match b.get(*i).copied() {
            Some(b'*') => {
                *i += 1;
                v *= parse_pow(b, i, vars)?;
            }
            Some(b'/') => {
                *i += 1;
                let d = parse_pow(b, i, vars)?;
                if d.abs() < 1e-15 {
                    return Err(ExprError("division by zero".into()));
                }
                v /= d;
            }
            Some(b'%') => {
                *i += 1;
                let d = parse_pow(b, i, vars)?;
                if d.abs() < 1e-15 {
                    return Err(ExprError("modulo by zero".into()));
                }
                // Euclidean remainder: result in [0, |d|) so negatives stay non-negative.
                v = v.rem_euclid(d);
            }
            _ => break,
        }
    }
    Ok(v)
}

fn parse_pow(b: &[u8], i: &mut usize, vars: ExprVars) -> Result<f64, ExprError> {
    let base = parse_unary(b, i, vars)?;
    skip_ws(b, i);
    if b.get(*i) == Some(&b'^') {
        *i += 1;
        let exp = parse_unary(b, i, vars)?;
        Ok(base.powf(exp))
    } else {
        Ok(base)
    }
}

fn parse_unary(b: &[u8], i: &mut usize, vars: ExprVars) -> Result<f64, ExprError> {
    skip_ws(b, i);
    if b.get(*i) == Some(&b'+') {
        *i += 1;
        return parse_unary(b, i, vars);
    }
    if b.get(*i) == Some(&b'-') {
        *i += 1;
        return Ok(-parse_unary(b, i, vars)?);
    }
    parse_primary(b, i, vars)
}

fn parse_primary(b: &[u8], i: &mut usize, vars: ExprVars) -> Result<f64, ExprError> {
    skip_ws(b, i);
    if *i >= b.len() {
        return Err(ExprError("unexpected end of expression".into()));
    }
    // number
    if b[*i].is_ascii_digit() || b[*i] == b'.' {
        return parse_number(b, i);
    }
    // paren
    if b[*i] == b'(' {
        *i += 1;
        let v = parse_expr(b, i, vars)?;
        skip_ws(b, i);
        if b.get(*i) != Some(&b')') {
            return Err(ExprError("expected ')'".into()));
        }
        *i += 1;
        return Ok(v);
    }
    // identifier / function
    if b[*i].is_ascii_alphabetic() || b[*i] == b'_' {
        let start = *i;
        *i += 1;
        while *i < b.len() && (b[*i].is_ascii_alphanumeric() || b[*i] == b'_') {
            *i += 1;
        }
        let name = std::str::from_utf8(&b[start..*i])
            .map_err(|_| ExprError("invalid identifier".into()))?
            .to_ascii_lowercase();
        skip_ws(b, i);
        if b.get(*i) == Some(&b'(') {
            *i += 1;
            let mut args = Vec::new();
            skip_ws(b, i);
            if b.get(*i) != Some(&b')') {
                loop {
                    args.push(parse_expr(b, i, vars)?);
                    skip_ws(b, i);
                    if b.get(*i) == Some(&b',') {
                        *i += 1;
                        continue;
                    }
                    break;
                }
            }
            skip_ws(b, i);
            if b.get(*i) != Some(&b')') {
                return Err(ExprError("expected ')' after function args".into()));
            }
            *i += 1;
            return call_fn(&name, &args);
        }
        return match name.as_str() {
            "t" => Ok(vars.t),
            "f" | "frame" => Ok(vars.f),
            "s" | "start" => Ok(vars.s),
            "x" => Ok(vars.x),
            "y" => Ok(vars.y),
            "r" => Ok(vars.r),
            "g" => Ok(vars.g),
            "b" => Ok(vars.b),
            "a" => Ok(vars.a),
            "pi" => Ok(std::f64::consts::PI),
            "tau" => Ok(std::f64::consts::TAU),
            "e" => Ok(std::f64::consts::E),
            _ => Err(ExprError(format!(
                "unknown variable '{name}' (t,f,s,x,y,r,g,b,a)"
            ))),
        };
    }
    Err(ExprError(format!(
        "unexpected character '{}'",
        b[*i] as char
    )))
}

fn parse_number(b: &[u8], i: &mut usize) -> Result<f64, ExprError> {
    let start = *i;
    while *i < b.len() && (b[*i].is_ascii_digit() || b[*i] == b'.') {
        *i += 1;
    }
    // scientific notation
    if *i < b.len() && (b[*i] == b'e' || b[*i] == b'E') {
        *i += 1;
        if *i < b.len() && (b[*i] == b'+' || b[*i] == b'-') {
            *i += 1;
        }
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
    }
    let s = std::str::from_utf8(&b[start..*i]).map_err(|_| ExprError("bad number".into()))?;
    s.parse::<f64>()
        .map_err(|_| ExprError(format!("invalid number '{s}'")))
}

fn call_fn(name: &str, args: &[f64]) -> Result<f64, ExprError> {
    let need = |n: usize| {
        if args.len() != n {
            Err(ExprError(format!(
                "{name}() expects {n} arg(s), got {}",
                args.len()
            )))
        } else {
            Ok(())
        }
    };
    match name {
        "sin" => {
            need(1)?;
            Ok(args[0].sin())
        }
        "cos" => {
            need(1)?;
            Ok(args[0].cos())
        }
        "tan" => {
            need(1)?;
            Ok(args[0].tan())
        }
        "abs" => {
            need(1)?;
            Ok(args[0].abs())
        }
        "sqrt" => {
            need(1)?;
            if args[0] < 0.0 {
                return Err(ExprError("sqrt of negative".into()));
            }
            Ok(args[0].sqrt())
        }
        "floor" => {
            need(1)?;
            Ok(args[0].floor())
        }
        "ceil" => {
            need(1)?;
            Ok(args[0].ceil())
        }
        "pow" => {
            need(2)?;
            Ok(args[0].powf(args[1]))
        }
        "min" => {
            need(2)?;
            Ok(args[0].min(args[1]))
        }
        "max" => {
            need(2)?;
            Ok(args[0].max(args[1]))
        }
        // Positive (Euclidean) modulus: mod(a, m) ∈ [0, |m|).
        "mod" | "modulus" => {
            need(2)?;
            if args[1].abs() < 1e-15 {
                return Err(ExprError("modulo by zero".into()));
            }
            Ok(args[0].rem_euclid(args[1]))
        }
        _ => Err(ExprError(format!("unknown function '{name}'"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_ops() {
        assert!((eval_expr("1+2*3", 0.0, 0.0).unwrap() - 7.0).abs() < 1e-9);
        assert!((eval_expr("t*100", 0.5, 0.0).unwrap() - 50.0).abs() < 1e-9);
        assert!((eval_expr("sin(0)", 0.0, 0.0).unwrap()).abs() < 1e-9);
        assert!((eval_expr("100+20*t", 1.0, 0.0).unwrap() - 120.0).abs() < 1e-9);
        assert!((eval_expr("abs(-3)", 0.0, 0.0).unwrap() - 3.0).abs() < 1e-9);
        assert!((eval_expr("mod(7, 5)", 0.0, 0.0).unwrap() - 2.0).abs() < 1e-9);
        assert!((eval_expr("(-1) % 5", 0.0, 0.0).unwrap() - 4.0).abs() < 1e-9);
        assert!((eval_expr("mod(-1, 5)", 0.0, 0.0).unwrap() - 4.0).abs() < 1e-9);
    }
}
