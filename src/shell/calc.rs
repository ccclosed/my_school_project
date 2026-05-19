use alloc::vec::Vec;

pub fn eval(input: &str) -> Result<i64, &'static str> {
    let tokens = tokenize(input)?;
    let rpn = shunting_yard(tokens)?;
    eval_rpn(&rpn)
}

#[derive(Clone, Copy, Debug)]
enum Tok {
    Num(i64),
    Op(u8),
    LParen,
    RParen,
}

fn tokenize(s: &str) -> Result<Vec<Tok>, &'static str> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || (c == b'-' && (out.is_empty() || matches!(out.last(), Some(Tok::Op(_)) | Some(Tok::LParen)))) {
            let start = i;
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let n: i64 = core::str::from_utf8(&bytes[start..i])
                .ok()
                .and_then(|t| t.parse().ok())
                .ok_or("bad number")?;
            out.push(Tok::Num(n));
            continue;
        }
        match c {
            b'+' | b'-' | b'*' | b'/' | b'%' => {
                out.push(Tok::Op(c));
                i += 1;
            }
            b'(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            b')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            _ => return Err("invalid token"),
        }
    }
    Ok(out)
}

fn prec(op: u8) -> i32 {
    match op {
        b'+' | b'-' => 1,
        b'*' | b'/' | b'%' => 2,
        _ => 0,
    }
}

fn shunting_yard(tokens: Vec<Tok>) -> Result<Vec<Tok>, &'static str> {
    let mut output = Vec::new();
    let mut ops: Vec<Tok> = Vec::new();
    for t in tokens {
        match t {
            Tok::Num(_) => output.push(t),
            Tok::Op(o) => {
                while let Some(Tok::Op(top)) = ops.last() {
                    if prec(*top) >= prec(o) {
                        output.push(ops.pop().unwrap());
                    } else {
                        break;
                    }
                }
                ops.push(Tok::Op(o));
            }
            Tok::LParen => ops.push(t),
            Tok::RParen => {
                while let Some(op) = ops.pop() {
                    if matches!(op, Tok::LParen) {
                        break;
                    }
                    output.push(op);
                }
            }
        }
    }
    while let Some(op) = ops.pop() {
        if matches!(op, Tok::LParen | Tok::RParen) {
            return Err("mismatched parens");
        }
        output.push(op);
    }
    Ok(output)
}

fn eval_rpn(tokens: &[Tok]) -> Result<i64, &'static str> {
    let mut stack: Vec<i64> = Vec::new();
    for t in tokens {
        match t {
            Tok::Num(n) => stack.push(*n),
            Tok::Op(op) => {
                let b = stack.pop().ok_or("stack underflow")?;
                let a = stack.pop().ok_or("stack underflow")?;
                let r = match op {
                    b'+' => a.checked_add(b),
                    b'-' => a.checked_sub(b),
                    b'*' => a.checked_mul(b),
                    b'/' => a.checked_div(b),
                    b'%' => a.checked_rem(b),
                    _ => None,
                }
                .ok_or("overflow/div0")?;
                stack.push(r);
            }
            _ => return Err("bad rpn"),
        }
    }
    if stack.len() != 1 {
        return Err("bad expression");
    }
    Ok(stack[0])
}
