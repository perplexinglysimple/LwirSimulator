use crate::bundle::Bundle;
use crate::cache::CacheConfig;
use crate::isa::{Opcode, Syllable};
use crate::layout::{
    default_arch_config, AesVariant, ArchConfig, FpVariant, ProcessorLayout, SlotSpec,
    TopologyConfig, UnitDecl, UnitKind,
};
use crate::program::Program;
use std::collections::HashMap;

#[derive(Clone, Debug)]
struct ParsedBundleLine {
    bundle_index: usize,
    line_no: usize,
    text: String,
}

const PROCESSOR_DOC_HINT: &str =
    "processor layout headers are mandatory; see docs/processor_layout_plan.md";

pub fn parse_program(text: &str) -> Result<Program, String> {
    let (layout, body_start) = parse_processor_header(text)?;
    if !layout.validate() {
        return Err("invalid processor layout; see docs/processor_layout_plan.md".to_string());
    }
    let body = text.lines().skip(body_start).collect::<Vec<_>>().join("\n");
    let parsed_lines = collect_lines(&body)?;
    let mut program = vec![
        Bundle::nop_bundle(layout.width);
        parsed_lines
            .iter()
            .map(|l| l.bundle_index)
            .max()
            .map_or(0, |i| i + 1)
    ];

    for parsed in parsed_lines {
        for part in parsed.text.split('|') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let (slot, syllable) = parse_instruction(part, parsed.line_no, layout.width)?;
            program[parsed.bundle_index].set_slot(slot, syllable);
        }
    }

    Ok(Program {
        layout,
        bundles: program,
    })
}

fn parse_processor_header(text: &str) -> Result<(ProcessorLayout, usize), String> {
    let mut first_line = None::<(usize, String)>;
    for (idx, raw_line) in text.lines().enumerate() {
        let line = strip_comment(raw_line).trim().to_string();
        if !line.is_empty() {
            first_line = Some((idx, line));
            break;
        }
    }

    let Some((start_idx, first)) = first_line else {
        return Err(format!(
            "missing `.processor {{ ... }}` header; {PROCESSOR_DOC_HINT}"
        ));
    };
    if first.starts_with(".width") {
        return Err(format!(
            "line {}: legacy `.width` header is no longer supported; {PROCESSOR_DOC_HINT}",
            start_idx + 1
        ));
    }
    if !first.starts_with(".processor") {
        return Err(format!(
            "line {}: expected `.processor {{ ... }}` header; {PROCESSOR_DOC_HINT}",
            start_idx + 1
        ));
    }

    let mut depth = 0isize;
    let mut saw_open = false;
    let mut block_lines = Vec::<(usize, String)>::new();
    for (idx, raw_line) in text.lines().enumerate().skip(start_idx) {
        let line_no = idx + 1;
        let line = strip_comment(raw_line).trim().to_string();
        if line.is_empty() {
            continue;
        }
        for ch in line.chars() {
            if ch == '{' {
                depth += 1;
                saw_open = true;
            } else if ch == '}' {
                depth -= 1;
                if depth < 0 {
                    return Err(format!("line {line_no}: unmatched `}}` in processor block"));
                }
            }
        }
        block_lines.push((line_no, line));
        if saw_open && depth == 0 {
            let layout = parse_processor_block(&block_lines)?;
            return Ok((layout, idx + 1));
        }
    }

    Err(format!(
        "line {}: unterminated `.processor` block",
        start_idx + 1
    ))
}

fn parse_processor_block(lines: &[(usize, String)]) -> Result<ProcessorLayout, String> {
    let mut width = None::<usize>;
    let mut units = Vec::<UnitDecl>::new();
    let mut slots = Vec::<Option<SlotSpec>>::new();
    let mut cache = CacheConfig::default_l1d();
    let mut saw_cache = false;
    let mut saw_topology = false;
    let mut topology_cpus = None::<usize>;
    let mut arch = default_arch_config();
    let mut section = "";

    for (line_no, raw_line) in lines {
        let mut line = raw_line.trim();
        if line.starts_with(".processor") {
            line = line.trim_start_matches(".processor").trim();
        }
        if line == "{" || line == "}" {
            continue;
        }
        if line.is_empty() {
            continue;
        }
        if line == "hardware" || line == "hardware {" {
            section = "hardware";
            continue;
        }
        if line == "layout slots" || line == "layout slots {" {
            section = "slots";
            continue;
        }
        if line.starts_with("cache") {
            saw_cache = true;
            section = "cache";
            parse_cache_fields(line, &mut cache)?;
            continue;
        }
        if line.starts_with("arch") {
            section = "arch";
            parse_arch_fields(line, &mut arch)?;
            continue;
        }
        if line.starts_with("topology") {
            saw_topology = true;
            section = "topology";
            if let Some(cpus) = parse_topology_cpus(line)? {
                topology_cpus = Some(cpus);
            }
            continue;
        }
        if line == "}" {
            section = "";
            continue;
        }

        if let Some(rest) = line.strip_prefix("width") {
            let value = rest.trim();
            width = Some(
                value
                    .parse::<usize>()
                    .map_err(|_| format!("line {line_no}: invalid processor width `{value}`"))?,
            );
            continue;
        }

        match section {
            "hardware" => {
                let decl = parse_unit_decl(line, *line_no)?;
                if units.iter().any(|u| u.name == decl.name) {
                    return Err(format!(
                        "line {line_no}: duplicate hardware unit `{}`",
                        decl.name
                    ));
                }
                units.push(decl);
            }
            "slots" => {
                let (slot, spec) = parse_slot_spec(line, *line_no)?;
                if slots.len() <= slot {
                    slots.resize_with(slot + 1, || None);
                }
                if slots[slot].is_some() {
                    return Err(format!("line {line_no}: duplicate layout slot {slot}"));
                }
                slots[slot] = Some(spec);
            }
            "topology" => {
                if let Some(cpus) = parse_topology_cpus(line)? {
                    topology_cpus = Some(cpus);
                }
            }
            "arch" => {
                parse_arch_fields(line, &mut arch)?;
            }
            "cache" => {
                parse_cache_fields(line, &mut cache)?;
            }
            _ => {
                return Err(format!(
                    "line {line_no}: unexpected processor layout directive `{line}`"
                ))
            }
        }
    }

    let width = width.ok_or_else(|| "processor block missing `width`".to_string())?;
    let mut final_slots = Vec::new();
    for slot in 0..slots.len() {
        let Some(spec) = slots[slot].take() else {
            return Err(format!("processor layout missing slot {slot}"));
        };
        final_slots.push(spec);
    }
    let layout = ProcessorLayout {
        width,
        units,
        slots: final_slots,
        arch,
        cache,
        topology: TopologyConfig {
            cpus: topology_cpus.unwrap_or(1),
        },
    };
    if !saw_cache {
        return Err("processor block missing stage-0 `cache { }` placeholder".to_string());
    }
    if !layout.cache.validate() {
        return Err("invalid direct-mapped L1D cache configuration".to_string());
    }
    if !saw_topology {
        return Err("processor block missing `topology { cpus N }` declaration".to_string());
    }
    Ok(layout)
}

fn parse_unit_decl(line: &str, line_no: usize) -> Result<UnitDecl, String> {
    let rest = line
        .strip_prefix("unit")
        .ok_or_else(|| format!("line {line_no}: expected `unit <name> = <kind>`"))?
        .trim();
    let (name, kind) = rest
        .split_once('=')
        .ok_or_else(|| format!("line {line_no}: expected `unit <name> = <kind>`"))?;
    let name = name.trim();
    if !is_identifier(name) {
        return Err(format!("line {line_no}: invalid unit name `{name}`"));
    }
    let (kind, latency) = parse_unit_kind(kind.trim(), line_no)?;
    Ok(UnitDecl {
        name: name.to_string(),
        kind,
        latency,
    })
}

fn parse_unit_kind(text: &str, line_no: usize) -> Result<(UnitKind, Option<u32>), String> {
    match text {
        "integer_alu" => return Ok((UnitKind::IntegerAlu, None)),
        "memory" => return Ok((UnitKind::Memory, None)),
        "control" => return Ok((UnitKind::Control, None)),
        "multiplier" => return Ok((UnitKind::Multiplier, None)),
        _ => {}
    }

    let (family, body) = text
        .split_once('{')
        .ok_or_else(|| format!("line {line_no}: unknown unit kind `{text}`"))?;
    let family = family.trim();
    let body = body
        .trim()
        .strip_suffix('}')
        .ok_or_else(|| format!("line {line_no}: expected `}}` to close `{family}` unit"))?
        .trim();
    let parts = body.split_whitespace().collect::<Vec<_>>();
    let mut variant = None::<&str>;
    let mut latency = None::<u32>;
    let mut i = 0usize;
    while i < parts.len() {
        match parts[i] {
            "variant" => {
                let value = parts.get(i + 1).ok_or_else(|| {
                    format!("line {line_no}: `{family}` unit missing variant value")
                })?;
                variant = Some(value);
                i += 2;
            }
            "latency" => {
                let value = parts.get(i + 1).ok_or_else(|| {
                    format!("line {line_no}: `{family}` unit missing latency value")
                })?;
                latency = Some(value.parse::<u32>().map_err(|_| {
                    format!("line {line_no}: invalid `{family}` unit latency `{value}`")
                })?);
                i += 2;
            }
            other => {
                return Err(format!(
                    "line {line_no}: unexpected `{family}` unit field `{other}`"
                ))
            }
        }
    }

    match family {
        "fp" => {
            let variant = match variant
                .ok_or_else(|| format!("line {line_no}: `fp` unit missing variant"))?
            {
                "fp32" => FpVariant::Fp32,
                "fp64" => FpVariant::Fp64,
                "fp64_fma" => FpVariant::Fp64Fma,
                other => return Err(format!("line {line_no}: unknown fp variant `{other}`")),
            };
            Ok((UnitKind::Fp(variant), latency))
        }
        "aes" => {
            let variant = match variant
                .ok_or_else(|| format!("line {line_no}: `aes` unit missing variant"))?
            {
                "aes_ni" => AesVariant::AesNi,
                other => return Err(format!("line {line_no}: unknown aes variant `{other}`")),
            };
            Ok((UnitKind::Aes(variant), latency))
        }
        _ => Err(format!("line {line_no}: unknown unit kind `{family}`")),
    }
}

fn parse_slot_spec(line: &str, line_no: usize) -> Result<(usize, SlotSpec), String> {
    let (slot, rhs) = line
        .split_once('=')
        .ok_or_else(|| format!("line {line_no}: expected `<slot> = {{ unit, ... }}`"))?;
    let slot = slot
        .trim()
        .parse::<usize>()
        .map_err(|_| format!("line {line_no}: invalid slot index `{}`", slot.trim()))?;
    let rhs = rhs.trim();
    if !(rhs.starts_with('{') && rhs.ends_with('}')) {
        return Err(format!("line {line_no}: expected unit set `{{ ... }}`"));
    }
    let inner = &rhs[1..rhs.len() - 1];
    let mut units = Vec::new();
    for part in inner.split(',') {
        let unit = part.trim();
        if unit.is_empty() {
            continue;
        }
        if !is_identifier(unit) {
            return Err(format!("line {line_no}: invalid unit reference `{unit}`"));
        }
        units.push(unit.to_string());
    }
    Ok((slot, SlotSpec { units }))
}

fn parse_topology_cpus(line: &str) -> Result<Option<usize>, String> {
    let cleaned = line.replace('{', " ").replace('}', " ");
    let parts = cleaned.split_whitespace().collect::<Vec<_>>();
    for pair in parts.windows(2) {
        if pair[0] == "cpus" {
            return pair[1]
                .parse::<usize>()
                .map(Some)
                .map_err(|_| format!("invalid topology cpu count `{}`", pair[1]));
        }
    }
    Ok(None)
}

fn parse_arch_fields(line: &str, arch: &mut ArchConfig) -> Result<(), String> {
    let cleaned = line.replace('{', " ").replace('}', " ");
    let parts = cleaned.split_whitespace().collect::<Vec<_>>();
    let mut i = 0usize;
    while i + 1 < parts.len() {
        match parts[i] {
            "gprs" => {
                arch.gprs = parts[i + 1]
                    .parse::<usize>()
                    .map_err(|_| format!("invalid architectural GPR count `{}`", parts[i + 1]))?;
                i += 2;
            }
            "preds" => {
                arch.preds = parts[i + 1].parse::<usize>().map_err(|_| {
                    format!("invalid architectural predicate count `{}`", parts[i + 1])
                })?;
                i += 2;
            }
            "memory" | "memory_bytes" => {
                arch.memory_bytes = parts[i + 1]
                    .parse::<usize>()
                    .map_err(|_| format!("invalid architectural memory size `{}`", parts[i + 1]))?;
                i += 2;
            }
            "arch" => {
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
    Ok(())
}

fn parse_cache_fields(line: &str, cache: &mut CacheConfig) -> Result<(), String> {
    let cleaned = line.replace('{', " ").replace('}', " ");
    let parts = cleaned.split_whitespace().collect::<Vec<_>>();
    let mut i = 0usize;
    while i + 1 < parts.len() {
        match parts[i] {
            "line_bytes" => {
                cache.line_bytes = parts[i + 1]
                    .parse::<usize>()
                    .map_err(|_| format!("invalid L1D line byte count `{}`", parts[i + 1]))?;
                i += 2;
            }
            "capacity" | "capacity_bytes" => {
                cache.capacity_bytes = parts[i + 1]
                    .parse::<usize>()
                    .map_err(|_| format!("invalid L1D capacity `{}`", parts[i + 1]))?;
                i += 2;
            }
            "associativity" => {
                cache.associativity = parts[i + 1]
                    .parse::<usize>()
                    .map_err(|_| format!("invalid L1D associativity `{}`", parts[i + 1]))?;
                i += 2;
            }
            "hit_latency" => {
                cache.hit_latency = parts[i + 1]
                    .parse::<u32>()
                    .map_err(|_| format!("invalid L1D hit latency `{}`", parts[i + 1]))?;
                i += 2;
            }
            "miss_latency" => {
                cache.miss_latency = parts[i + 1]
                    .parse::<u32>()
                    .map_err(|_| format!("invalid L1D miss latency `{}`", parts[i + 1]))?;
                i += 2;
            }
            "writeback_latency" => {
                cache.writeback_latency = parts[i + 1]
                    .parse::<u32>()
                    .map_err(|_| format!("invalid L1D writeback latency `{}`", parts[i + 1]))?;
                i += 2;
            }
            "cache" | "l1d" | "write_policy" => {
                i += if parts[i] == "write_policy" { 2 } else { 1 };
            }
            _ => {
                i += 1;
            }
        }
    }
    Ok(())
}

fn collect_lines(text: &str) -> Result<Vec<ParsedBundleLine>, String> {
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
            return Err(format!("line {line_no}: legacy `.width` header is no longer supported; {PROCESSOR_DOC_HINT}"));
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
        return Err(format!(
            "line {label_line_no}: label `{label}` does not apply to any bundle"
        ));
    }

    for parsed_line in &mut parsed {
        parsed_line.text = resolve_labels(&parsed_line.text, &labels, parsed_line.line_no)?;
    }

    Ok(parsed)
}

fn normalize_block_instruction_line(line: &str, line_no: usize) -> Result<String, String> {
    let Some(colon) = line.find(':') else {
        return Err(format!(
            "line {line_no}: expected `<slot>: <opcode> ...` inside bundle block"
        ));
    };
    let slot = line[..colon].trim();
    if slot.is_empty() {
        return Err(format!("line {line_no}: missing slot before `:`"));
    }
    let rest = line[colon + 1..].trim();
    if rest.is_empty() {
        return Err(format!(
            "line {line_no}: missing instruction after `{slot}:`"
        ));
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

fn resolve_labels(
    line: &str,
    labels: &HashMap<String, usize>,
    line_no: usize,
) -> Result<String, String> {
    Ok(replace_labels_in_line(line, labels, line_no)?)
}

fn replace_labels_in_line(
    line: &str,
    labels: &HashMap<String, usize>,
    line_no: usize,
) -> Result<String, String> {
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

fn resolve_token(
    token: &str,
    labels: &HashMap<String, usize>,
    line_no: usize,
) -> Result<String, String> {
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
        "i0" | "i1"
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
            | "fpadd32"
            | "fp_add32"
            | "fpmul32"
            | "fp_mul32"
            | "fpadd64"
            | "fp_add64"
            | "fpmul64"
            | "fp_mul64"
            | "aesenc"
            | "aes_enc"
            | "aesdec"
            | "aes_dec"
            | "nop"
    )
}

fn parse_instruction(
    line: &str,
    line_no: usize,
    width: usize,
) -> Result<(usize, Syllable), String> {
    let normalized = normalize_instruction_text(line);
    let mut tokens: Vec<&str> = normalized.split_whitespace().collect();
    if tokens.len() < 2 {
        return Err(format!("line {line_no}: expected `<slot> <opcode> ...`"));
    }

    let slot = parse_slot(tokens[0], line_no)?;
    if slot >= width {
        return Err(format!(
            "line {line_no}: slot {slot} out of range for width {width}"
        ));
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
        Opcode::Add
        | Opcode::Sub
        | Opcode::And
        | Opcode::Or
        | Opcode::Xor
        | Opcode::Shl
        | Opcode::Srl
        | Opcode::Sra
        | Opcode::Mul
        | Opcode::MulH
        | Opcode::FpAdd32
        | Opcode::FpMul32
        | Opcode::FpAdd64
        | Opcode::FpMul64
        | Opcode::AesEnc
        | Opcode::AesDec => {
            expect_arity(args, 3, line_no, opcode)?;
            syllable.dst = Some(parse_gpr(args[0], line_no)?);
            syllable.src = [
                Some(parse_gpr(args[1], line_no)?),
                Some(parse_gpr(args[2], line_no)?),
            ];
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
            syllable.src = [
                Some(parse_gpr(args[1], line_no)?),
                Some(parse_gpr(args[2], line_no)?),
            ];
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
            syllable.src = [
                Some(parse_pred(args[1], line_no)?),
                Some(parse_pred(args[2], line_no)?),
            ];
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
        "fpadd32" | "fp_add32" => Opcode::FpAdd32,
        "fpmul32" | "fp_mul32" => Opcode::FpMul32,
        "fpadd64" | "fp_add64" => Opcode::FpAdd64,
        "fpmul64" | "fp_mul64" => Opcode::FpMul64,
        "aesenc" | "aes_enc" => Opcode::AesEnc,
        "aesdec" | "aes_dec" => Opcode::AesDec,
        "nop" => Opcode::Nop,
        _ => return Err(format!("line {line_no}: unknown opcode `{token}`")),
    };
    Ok(opcode)
}

fn parse_load_like_operands(
    args: &[&str],
    line_no: usize,
    opcode: Opcode,
) -> Result<(usize, usize, i64), String> {
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

fn parse_store_operands(
    args: &[&str],
    line_no: usize,
    opcode: Opcode,
) -> Result<(usize, usize, i64), String> {
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
        return Err(format!(
            "line {line_no}: expected GPR like `r3`, got `{token}`"
        ));
    };
    rest.parse::<usize>()
        .map_err(|_| format!("line {line_no}: invalid GPR `{token}`"))
}

fn parse_pred(token: &str, line_no: usize) -> Result<usize, String> {
    let Some(rest) = token.strip_prefix('p') else {
        return Err(format!(
            "line {line_no}: expected predicate like `p1`, got `{token}`"
        ));
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
        token
            .parse::<i64>()
            .map_err(|_| format!("invalid immediate `{token}`"))
    }
}

fn normalize_instruction_text(line: &str) -> String {
    line.replace(',', " ").replace('+', " + ")
}

fn expect_arity(
    args: &[&str],
    expected: usize,
    line_no: usize,
    opcode: Opcode,
) -> Result<(), String> {
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

    fn processor_header(width: usize) -> String {
        let mut slots = String::new();
        for slot in 0..width {
            let units = match slot % 4 {
                0 | 1 => "alu",
                2 => "mem",
                _ => "ctrl, mul",
            };
            slots.push_str(&format!("    {slot} = {{ {units} }}\n"));
        }
        format!(
            ".processor {{\n  width {width}\n  hardware {{\n    unit alu = integer_alu\n    unit mem = memory\n    unit ctrl = control\n    unit mul = multiplier\n  }}\n  layout slots {{\n{slots}  }}\n  cache {{ }}\n  topology {{ cpus 1 }}\n}}\n"
        )
    }

    #[test]
    fn parses_and_executes_text_program_with_labels() {
        let source = format!(
            "{}{}",
            processor_header(W),
            r#"
start: i0 mov_imm r1, 6 | i1 mov_imm r2, 7
       x mul r3, r1, r2
       m store_d r0, r3, 0x100
done:  x ret
"#
        );

        let program = parse_program(&source).expect("program should parse");
        let mut latencies = LatencyTable::default();
        latencies.set(Opcode::Mul, 5);
        let mut cpu = CpuState::new_for_layout(&program.layout, latencies);

        while cpu.step(&program.layout, &program.bundles) {}

        assert!(cpu.halted);
        assert_eq!(cpu.read_gpr(3), 42);
        let stored = u64::from_le_bytes(cpu.memory[0x100..0x108].try_into().unwrap());
        assert_eq!(stored, 42);
    }

    #[test]
    fn rejects_unknown_label() {
        let source = format!("{}{}", processor_header(W), "start: x jump missing_label");
        let err = parse_program(&source).expect_err("program should fail");
        assert!(err.contains("unknown label"));
    }

    #[test]
    fn parses_block_style_assembly() {
        let source = r#"
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
        let source = format!("{}{}", processor_header(W), source);

        let program = parse_program(&source).expect("program should parse");
        let mut cpu = CpuState::new_for_layout(&program.layout, LatencyTable::default());

        while cpu.step(&program.layout, &program.bundles) {}

        assert!(cpu.halted);
        assert_eq!(cpu.read_gpr(3), 30);
        assert_eq!(cpu.read_gpr(4), 1);
        let stored = u64::from_le_bytes(cpu.memory[0x100..0x108].try_into().unwrap());
        assert_eq!(stored, 30);
    }
}
