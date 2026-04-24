use crate::bundle::Bundle;
use crate::isa::{Opcode, Syllable};
use std::collections::HashMap;

#[derive(Clone, Debug)]
struct ParsedBundleLine {
    bundle_index: usize,
    line_no: usize,
    text: String,
}

pub fn parse_program<const W: usize>(text: &str) -> Result<Vec<Bundle<W>>, String> {
    let parsed_lines = collect_lines::<W>(text)?;
    let mut program =
        vec![Bundle::<W>::nop_bundle(); parsed_lines.iter().map(|l| l.bundle_index).max().map_or(0, |i| i + 1)];

    for parsed in parsed_lines {
        for part in parsed.text.split('|') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let (slot, syllable) = parse_instruction::<W>(part, parsed.line_no)?;
            program[parsed.bundle_index].set_slot(slot, syllable);
        }
    }

    Ok(program)
}

fn collect_lines<const W: usize>(text: &str) -> Result<Vec<ParsedBundleLine>, String> {
    let mut labels = HashMap::<String, usize>::new();
    let mut parsed = Vec::<ParsedBundleLine>::new();
    let mut bundle_index = 0usize;
    let mut pending_labels = Vec::<(String, usize)>::new();
    let mut in_block = false;
    let mut block_parts = Vec::<String>::new();
    let mut block_line_no = 0usize;

    for (idx, raw_line) in text.lines().enumerate() {
        let line_no = idx + 1;
        let mut line = strip_comment(raw_line).trim().to_string();
        if line.is_empty() {
            continue;
        }

        if line.starts_with(".width") {
            let width = line
                .strip_prefix(".width")
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| format!("line {line_no}: expected `.width <n>`"))?;
            let parsed_width = width
                .parse::<usize>()
                .map_err(|_| format!("line {line_no}: invalid width `{width}`"))?;
            if parsed_width != W {
                return Err(format!(
                    "line {line_no}: source declares width {parsed_width}, parser is instantiated for width {W}"
                ));
            }
            continue;
        }

        if in_block {
            if line == "}" {
                parsed.push(ParsedBundleLine {
                    bundle_index,
                    line_no: block_line_no,
                    text: block_parts.join(" | "),
                });
                bundle_index += 1;
                in_block = false;
                block_parts.clear();
                continue;
            }

            block_parts.push(normalize_block_instruction_line(&line, line_no)?);
            continue;
        }

        while let Some((label, rest)) = split_label_prefix(&line) {
            pending_labels.push((label, line_no));
            line = rest.to_string();
            if line.trim().is_empty() {
                break;
            }
        }

        if line.trim().is_empty() {
            continue;
        }

        if line == "{" {
            for (label, label_line_no) in pending_labels.drain(..) {
                if labels.insert(label.clone(), bundle_index).is_some() {
                    return Err(format!("line {label_line_no}: duplicate label `{label}`"));
                }
            }
            in_block = true;
            block_line_no = line_no;
            block_parts.clear();
            continue;
        }

        for (label, label_line_no) in pending_labels.drain(..) {
            if labels.insert(label.clone(), bundle_index).is_some() {
                return Err(format!("line {label_line_no}: duplicate label `{label}`"));
            }
        }

        parsed.push(ParsedBundleLine {
            bundle_index,
            line_no,
            text: line.trim().to_string(),
        });
        bundle_index += 1;
    }

    if in_block {
        return Err(format!("line {block_line_no}: unterminated bundle block"));
    }

    if let Some((label, label_line_no)) = pending_labels.first() {
        return Err(format!("line {label_line_no}: label `{label}` does not apply to any bundle"));
    }

    for parsed_line in &mut parsed {
        parsed_line.text = resolve_labels(&parsed_line.text, &labels, parsed_line.line_no)?;
    }

    Ok(parsed)
}

fn normalize_block_instruction_line(line: &str, line_no: usize) -> Result<String, String> {
    let Some(colon) = line.find(':') else {
        return Err(format!("line {line_no}: expected `<slot>: <opcode> ...` inside bundle block"));
    };
    let slot = line[..colon].trim();
    if slot.is_empty() {
        return Err(format!("line {line_no}: missing slot before `:`"));
    }
    let rest = line[colon + 1..].trim();
    if rest.is_empty() {
        return Err(format!("line {line_no}: missing instruction after `{slot}:`"));
    }
    Ok(format!("{slot} {rest}"))
}

fn split_label_prefix(line: &str) -> Option<(String, &str)> {
    let colon = line.find(':')?;
    let label = line[..colon].trim();
    if !is_identifier(label) {
        return None;
    }
    Some((label.to_string(), &line[colon + 1..]))
}

fn is_identifier(token: &str) -> bool {
    let mut chars = token.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn resolve_labels(line: &str, labels: &HashMap<String, usize>, line_no: usize) -> Result<String, String> {
    Ok(replace_labels_in_line(line, labels, line_no)?)
}

fn replace_labels_in_line(line: &str, labels: &HashMap<String, usize>, line_no: usize) -> Result<String, String> {
    let mut out = String::new();
    let mut token = String::new();
    for ch in line.chars() {
        if ch.is_whitespace() || ch == ',' || ch == '|' {
            if !token.is_empty() {
                out.push_str(&resolve_token(&token, labels, line_no)?);
                token.clear();
            }
            out.push(ch);
        } else {
            token.push(ch);
        }
    }
    if !token.is_empty() {
        out.push_str(&resolve_token(&token, labels, line_no)?);
    }
    Ok(out)
}

fn resolve_token(token: &str, labels: &HashMap<String, usize>, line_no: usize) -> Result<String, String> {
    if looks_like_label_ref(token) {
        match labels.get(token) {
            Some(target) => Ok(target.to_string()),
            None => Err(format!("line {line_no}: unknown label `{token}`")),
        }
    } else {
        Ok(token.to_string())
    }
}

fn looks_like_label_ref(piece: &str) -> bool {
    is_identifier(piece)
        && !piece.starts_with('r')
        && !piece.starts_with('p')
        && !piece.starts_with('[')
        && !is_reserved_token(piece)
        && parse_i64(piece).is_err()
        && piece != "!"
}

fn is_reserved_token(token: &str) -> bool {
    matches!(
        token.to_ascii_lowercase().as_str(),
        "i0"
            | "i1"
            | "m"
            | "x"
            | "add"
            | "sub"
            | "and"
            | "or"
            | "xor"
            | "shl"
            | "srl"
            | "sra"
            | "mov"
            | "mov_imm"
            | "movimm"
            | "movi"
            | "cmpeq"
            | "cmp_eq"
            | "cmplt"
            | "cmp_lt"
            | "cmpult"
            | "cmp_ult"
            | "loadb"
            | "load_b"
            | "ldb"
            | "loadh"
            | "load_h"
            | "ldh"
            | "loadw"
            | "load_w"
            | "ldw"
            | "loadd"
            | "load_d"
            | "ldd"
            | "storeb"
            | "store_b"
            | "stb"
            | "storeh"
            | "store_h"
            | "sth"
            | "storew"
            | "store_w"
            | "stw"
            | "stored"
            | "store_d"
            | "std"
            | "lea"
            | "prefetch"
            | "mul"
            | "mulh"
            | "mul_h"
            | "branch"
            | "br"
            | "jump"
            | "jmp"
            | "call"
            | "ret"
            | "pand"
            | "p_and"
            | "por"
            | "p_or"
            | "pxor"
            | "p_xor"
            | "pnot"
            | "p_not"
            | "nop"
    )
}

fn parse_instruction<const W: usize>(line: &str, line_no: usize) -> Result<(usize, Syllable), String> {
    let normalized = normalize_instruction_text(line);
    let mut tokens: Vec<&str> = normalized.split_whitespace().collect();
    if tokens.len() < 2 {
        return Err(format!("line {line_no}: expected `<slot> <opcode> ...`"));
    }

    let slot = parse_slot(tokens[0], line_no)?;
    if slot >= W {
        return Err(format!("line {line_no}: slot {slot} out of range for width {W}"));
    }
    tokens.remove(0);

    let mut predicate = 0usize;
    let mut pred_negated = false;
    if let Some(guard) = tokens.first().copied() {
        if let Some((pred, negated)) = parse_guard(guard, line_no)? {
            predicate = pred;
            pred_negated = negated;
            tokens.remove(0);
        }
    }

    if tokens.is_empty() {
        return Err(format!("line {line_no}: missing opcode"));
    }

    let opcode = parse_opcode(tokens[0], line_no)?;
    let args = &tokens[1..];
    if opcode == Opcode::Branch {
        if predicate != 0 || pred_negated {
            return Err(format!("line {line_no}: branch uses its predicate operand directly; guard syntax is not supported here"));
        }
        parse_branch(slot, args, line_no)
    } else {
        let mut syllable = parse_non_branch(opcode, args, line_no)?;
        syllable.predicate = predicate;
        syllable.pred_negated = pred_negated;
        Ok((slot, syllable))
    }
}

fn parse_branch(slot: usize, args: &[&str], line_no: usize) -> Result<(usize, Syllable), String> {
    if args.len() != 2 {
        return Err(format!("line {line_no}: branch expects `<pred> <target>`"));
    }
    let (predicate, pred_negated) = parse_pred_ref(args[0], line_no)?;
    let target = parse_i64(args[1]).map_err(|e| format!("line {line_no}: {e}"))?;
    Ok((
        slot,
        Syllable {
            opcode: Opcode::Branch,
            dst: None,
            src: [None, None],
            imm: target,
            predicate,
            pred_negated,
        },
    ))
}

fn parse_non_branch(opcode: Opcode, args: &[&str], line_no: usize) -> Result<Syllable, String> {
    let mut syllable = Syllable::nop();
    syllable.opcode = opcode;

    match opcode {
        Opcode::Add | Opcode::Sub | Opcode::And | Opcode::Or | Opcode::Xor
        | Opcode::Shl | Opcode::Srl | Opcode::Sra | Opcode::Mul | Opcode::MulH => {
            expect_arity(args, 3, line_no, opcode)?;
            syllable.dst = Some(parse_gpr(args[0], line_no)?);
            syllable.src = [Some(parse_gpr(args[1], line_no)?), Some(parse_gpr(args[2], line_no)?)];
        }
        Opcode::Mov => {
            expect_arity(args, 2, line_no, opcode)?;
            syllable.dst = Some(parse_gpr(args[0], line_no)?);
            syllable.src = [Some(parse_gpr(args[1], line_no)?), None];
        }
        Opcode::MovImm => {
            expect_arity(args, 2, line_no, opcode)?;
            syllable.dst = Some(parse_gpr(args[0], line_no)?);
            syllable.imm = parse_i64(args[1]).map_err(|e| format!("line {line_no}: {e}"))?;
        }
        Opcode::CmpEq | Opcode::CmpLt | Opcode::CmpUlt => {
            expect_arity(args, 3, line_no, opcode)?;
            syllable.dst = Some(parse_pred(args[0], line_no)?);
            syllable.src = [Some(parse_gpr(args[1], line_no)?), Some(parse_gpr(args[2], line_no)?)];
        }
        Opcode::LoadB | Opcode::LoadH | Opcode::LoadW | Opcode::LoadD | Opcode::Lea => {
            let (dst, base, imm) = parse_load_like_operands(args, line_no, opcode)?;
            syllable.dst = Some(dst);
            syllable.src = [Some(base), None];
            syllable.imm = imm;
        }
        Opcode::StoreB | Opcode::StoreH | Opcode::StoreW | Opcode::StoreD => {
            let (base, src, imm) = parse_store_operands(args, line_no, opcode)?;
            syllable.src = [Some(base), Some(src)];
            syllable.imm = imm;
        }
        Opcode::Prefetch => {
            expect_arity(args, 2, line_no, opcode)?;
            syllable.src = [Some(parse_gpr(args[0], line_no)?), None];
            syllable.imm = parse_i64(args[1]).map_err(|e| format!("line {line_no}: {e}"))?;
        }
        Opcode::Jump | Opcode::Call => {
            expect_arity(args, 1, line_no, opcode)?;
            syllable.imm = parse_i64(args[0]).map_err(|e| format!("line {line_no}: {e}"))?;
        }
        Opcode::Ret | Opcode::Nop => {
            expect_arity(args, 0, line_no, opcode)?;
        }
        Opcode::PAnd | Opcode::POr | Opcode::PXor => {
            expect_arity(args, 3, line_no, opcode)?;
            syllable.dst = Some(parse_pred(args[0], line_no)?);
            syllable.src = [Some(parse_pred(args[1], line_no)?), Some(parse_pred(args[2], line_no)?)];
        }
        Opcode::PNot => {
            expect_arity(args, 2, line_no, opcode)?;
            syllable.dst = Some(parse_pred(args[0], line_no)?);
            syllable.src = [Some(parse_pred(args[1], line_no)?), None];
        }
        Opcode::Branch => unreachable!(),
    }

    Ok(syllable)
}

fn parse_slot(token: &str, line_no: usize) -> Result<usize, String> {
    match token.to_ascii_lowercase().as_str() {
        "i0" => Ok(0),
        "i1" => Ok(1),
        "m" => Ok(2),
        "x" => Ok(3),
        _ => token
            .parse::<usize>()
            .map_err(|_| format!("line {line_no}: invalid slot `{token}`")),
    }
}

fn parse_guard(token: &str, line_no: usize) -> Result<Option<(usize, bool)>, String> {
    if !(token.starts_with('[') && token.ends_with(']')) {
        return Ok(None);
    }
    let inner = &token[1..token.len() - 1];
    let (pred, negated) = parse_pred_ref(inner, line_no)?;
    Ok(Some((pred, negated)))
}

fn parse_pred_ref(token: &str, line_no: usize) -> Result<(usize, bool), String> {
    let (negated, pred_token) = if let Some(rest) = token.strip_prefix('!') {
        (true, rest)
    } else {
        (false, token)
    };
    Ok((parse_pred(pred_token, line_no)?, negated))
}

fn parse_opcode(token: &str, line_no: usize) -> Result<Opcode, String> {
    let normalized = token.to_ascii_lowercase();
    let opcode = match normalized.as_str() {
        "add" => Opcode::Add,
        "sub" => Opcode::Sub,
        "and" => Opcode::And,
        "or" => Opcode::Or,
        "xor" => Opcode::Xor,
        "shl" => Opcode::Shl,
        "srl" => Opcode::Srl,
        "sra" => Opcode::Sra,
        "mov" => Opcode::Mov,
        "mov_imm" | "movimm" | "movi" => Opcode::MovImm,
        "cmpeq" | "cmp_eq" => Opcode::CmpEq,
        "cmplt" | "cmp_lt" => Opcode::CmpLt,
        "cmpult" | "cmp_ult" => Opcode::CmpUlt,
        "loadb" | "load_b" | "ldb" => Opcode::LoadB,
        "loadh" | "load_h" | "ldh" => Opcode::LoadH,
        "loadw" | "load_w" | "ldw" => Opcode::LoadW,
        "loadd" | "load_d" | "ldd" => Opcode::LoadD,
        "storeb" | "store_b" | "stb" => Opcode::StoreB,
        "storeh" | "store_h" | "sth" => Opcode::StoreH,
        "storew" | "store_w" | "stw" => Opcode::StoreW,
        "stored" | "store_d" | "std" => Opcode::StoreD,
        "lea" => Opcode::Lea,
        "prefetch" => Opcode::Prefetch,
        "mul" => Opcode::Mul,
        "mulh" | "mul_h" => Opcode::MulH,
        "branch" | "br" => Opcode::Branch,
        "jump" | "jmp" => Opcode::Jump,
        "call" => Opcode::Call,
        "ret" => Opcode::Ret,
        "pand" | "p_and" => Opcode::PAnd,
        "por" | "p_or" => Opcode::POr,
        "pxor" | "p_xor" => Opcode::PXor,
        "pnot" | "p_not" => Opcode::PNot,
        "nop" => Opcode::Nop,
        _ => return Err(format!("line {line_no}: unknown opcode `{token}`")),
    };
    Ok(opcode)
}

fn parse_load_like_operands(args: &[&str], line_no: usize, opcode: Opcode) -> Result<(usize, usize, i64), String> {
    if args.len() == 3 {
        return Ok((
            parse_gpr(args[0], line_no)?,
            parse_gpr(args[1], line_no)?,
            parse_i64(args[2]).map_err(|e| format!("line {line_no}: {e}"))?,
        ));
    }

    if args.len() == 4 {
        let base = args[1]
            .strip_prefix('[')
            .ok_or_else(|| format!("line {line_no}: expected `[` to start memory operand"))?;
        let imm = args[3]
            .strip_suffix(']')
            .ok_or_else(|| format!("line {line_no}: expected `]` to end memory operand"))?;
        if args[2] != "+" {
            return Err(format!("line {line_no}: expected `+` in memory operand"));
        }
        return Ok((
            parse_gpr(args[0], line_no)?,
            parse_gpr(base, line_no)?,
            parse_i64(imm).map_err(|e| format!("line {line_no}: {e}"))?,
        ));
    }

    Err(format!(
        "line {line_no}: opcode `{:?}` expects `dst, base, imm` or `dst, [base + imm]`",
        opcode
    ))
}

fn parse_store_operands(args: &[&str], line_no: usize, opcode: Opcode) -> Result<(usize, usize, i64), String> {
    if args.len() == 3 {
        return Ok((
            parse_gpr(args[0], line_no)?,
            parse_gpr(args[1], line_no)?,
            parse_i64(args[2]).map_err(|e| format!("line {line_no}: {e}"))?,
        ));
    }

    if args.len() == 4 {
        let base = args[0]
            .strip_prefix('[')
            .ok_or_else(|| format!("line {line_no}: expected `[` to start memory operand"))?;
        let imm = args[2]
            .strip_suffix(']')
            .ok_or_else(|| format!("line {line_no}: expected `]` to end memory operand"))?;
        if args[1] != "+" {
            return Err(format!("line {line_no}: expected `+` in memory operand"));
        }
        return Ok((
            parse_gpr(base, line_no)?,
            parse_gpr(args[3], line_no)?,
            parse_i64(imm).map_err(|e| format!("line {line_no}: {e}"))?,
        ));
    }

    Err(format!(
        "line {line_no}: opcode `{:?}` expects `base, src, imm` or `[base + imm], src`",
        opcode
    ))
}

fn parse_gpr(token: &str, line_no: usize) -> Result<usize, String> {
    let Some(rest) = token.strip_prefix('r') else {
        return Err(format!("line {line_no}: expected GPR like `r3`, got `{token}`"));
    };
    rest.parse::<usize>()
        .map_err(|_| format!("line {line_no}: invalid GPR `{token}`"))
}

fn parse_pred(token: &str, line_no: usize) -> Result<usize, String> {
    let Some(rest) = token.strip_prefix('p') else {
        return Err(format!("line {line_no}: expected predicate like `p1`, got `{token}`"));
    };
    rest.parse::<usize>()
        .map_err(|_| format!("line {line_no}: invalid predicate `{token}`"))
}

fn parse_i64(token: &str) -> Result<i64, String> {
    if let Some(rest) = token.strip_prefix("-0x") {
        i64::from_str_radix(rest, 16)
            .map(|v| -v)
            .map_err(|_| format!("invalid immediate `{token}`"))
    } else if let Some(rest) = token.strip_prefix("0x") {
        i64::from_str_radix(rest, 16).map_err(|_| format!("invalid immediate `{token}`"))
    } else {
        token.parse::<i64>().map_err(|_| format!("invalid immediate `{token}`"))
    }
}

fn normalize_instruction_text(line: &str) -> String {
    line.replace(',', " ")
        .replace('+', " + ")
}

fn expect_arity(args: &[&str], expected: usize, line_no: usize, opcode: Opcode) -> Result<(), String> {
    if args.len() != expected {
        return Err(format!(
            "line {line_no}: opcode `{:?}` expects {expected} operand(s), got {}",
            opcode,
            args.len()
        ));
    }
    Ok(())
}

fn strip_comment(line: &str) -> &str {
    match line.find('#') {
        Some(idx) => &line[..idx],
        None => line,
    }
}

#[cfg(test)]
mod tests {
    use super::parse_program;
    use crate::cpu::CpuState;
    use crate::isa::Opcode;
    use crate::latency::LatencyTable;

    const W: usize = 4;

    #[test]
    fn parses_and_executes_text_program_with_labels() {
        let source = r#"
start: i0 mov_imm r1, 6 | i1 mov_imm r2, 7
       x mul r3, r1, r2
       m store_d r0, r3, 0x100
done:  x ret
"#;

        let program = parse_program::<W>(source).expect("program should parse");
        let mut latencies = LatencyTable::default();
        latencies.set(Opcode::Mul, 5);
        let mut cpu = CpuState::<W>::new(latencies);

        while cpu.step(&program) {}

        assert!(cpu.halted);
        assert_eq!(cpu.read_gpr(3), 42);
        let stored = u64::from_le_bytes(cpu.memory[0x100..0x108].try_into().unwrap());
        assert_eq!(stored, 42);
    }

    #[test]
    fn rejects_unknown_label() {
        let source = "start: x jump missing_label";
        let err = parse_program::<W>(source).expect_err("program should fail");
        assert!(err.contains("unknown label"));
    }

    #[test]
    fn parses_block_style_assembly() {
        let source = r#"
.width 4

start:
{
  I0: movi r1, 10
  I1: movi r2, 20
  M : nop
  X : nop
}

{
  I0: add r3, r1, r2
  I1: cmplt p1, r1, r2
  M : nop
  X : nop
}

{
  I0: [p1] movi r4, 1
  I1: [!p1] movi r4, 0
  M : std [r0 + 0x100], r3
  X : nop
}

{
  I0: nop
  I1: nop
  M : nop
  X : ret
}
"#;

        let program = parse_program::<W>(source).expect("program should parse");
        let mut cpu = CpuState::<W>::new(LatencyTable::default());

        while cpu.step(&program) {}

        assert!(cpu.halted);
        assert_eq!(cpu.read_gpr(3), 30);
        assert_eq!(cpu.read_gpr(4), 1);
        let stored = u64::from_le_bytes(cpu.memory[0x100..0x108].try_into().unwrap());
        assert_eq!(stored, 30);
    }
}
