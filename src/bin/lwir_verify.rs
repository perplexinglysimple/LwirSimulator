/// Standalone program verifier for LWIR assembly.
///
/// Statically checks a .lwir / .lwirasm file against the compiler contract
/// in docs/compiler_contract.md without running the program.
///
/// Exit codes:
///   0  — no violations found
///   1  — one or more contract violations found
///   2  — usage error or parse failure
use lwir_simulator::asm::parse_program;
use lwir_simulator::latency::LatencyTable;
use lwir_simulator::verifier::{verify_program, Rule};
use std::env;
use std::fs;
use std::process::ExitCode;

const DEFAULT_WIDTH: usize = 4;
const SUPPORTED_WIDTHS: &str = "4, 8, 16, 32, 64, 128, 256";

fn main() -> ExitCode {
    let mut args = env::args();
    let exe = args.next().unwrap_or_else(|| "lwir_verify".to_string());
    let Some(path) = args.next() else {
        eprintln!("usage: {exe} <program.lwir>");
        eprintln!("  Statically verifies a .lwir program against the compiler contract.");
        eprintln!("  Exit 0: clean  Exit 1: violations found  Exit 2: parse error");
        return ExitCode::from(2);
    };
    if args.next().is_some() {
        eprintln!("usage: {exe} <program.lwir>");
        return ExitCode::from(2);
    }

    let source = match fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: could not read `{path}`: {e}");
            return ExitCode::from(2);
        }
    };

    let width = match declared_width(&source) {
        Ok(Some(width)) => width,
        Ok(None) => DEFAULT_WIDTH,
        Err(e) => {
            eprintln!("error: parse failed for `{path}`: {e}");
            return ExitCode::from(2);
        }
    };

    match width {
        4 => verify_for_width::<4>(&path, &source),
        8 => verify_for_width::<8>(&path, &source),
        16 => verify_for_width::<16>(&path, &source),
        32 => verify_for_width::<32>(&path, &source),
        64 => verify_for_width::<64>(&path, &source),
        128 => verify_for_width::<128>(&path, &source),
        256 => verify_for_width::<256>(&path, &source),
        _ => {
            eprintln!("error: unsupported width {width}; supported widths are: {SUPPORTED_WIDTHS}");
            ExitCode::from(2)
        }
    }
}

fn verify_for_width<const W: usize>(path: &str, source: &str) -> ExitCode {
    let program = match parse_program::<W>(source) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: parse failed for `{path}`: {e}");
            return ExitCode::from(2);
        }
    };

    let latencies = LatencyTable::default();
    let diags = verify_program(&program, &latencies);

    println!("LWIR Verifier (W={W})");
    println!("Program : {path}");
    println!("Bundles : {}", program.len());

    if diags.is_empty() {
        println!("Result  : CLEAN — no contract violations found");
        return ExitCode::SUCCESS;
    }

    println!("Result  : {} violation(s) found", diags.len());
    println!();
    for d in &diags {
        let rule_tag = match d.rule {
            Rule::SlotOpcodeLegality   => "slot-opcode-legality",
            Rule::SameBundleGprRaw     => "same-bundle-gpr-raw",
            Rule::SameBundleGprWaw     => "same-bundle-gpr-waw",
            Rule::SameBundlePredHazard => "same-bundle-pred-hazard",
            Rule::GprReadyCycle        => "gpr-ready-cycle",
        };
        println!("[{rule_tag}] {}", d.message);
    }

    ExitCode::from(1)
}

fn declared_width(source: &str) -> Result<Option<usize>, String> {
    for (idx, raw_line) in source.lines().enumerate() {
        let line_no = idx + 1;
        let line = strip_comment(raw_line).trim();
        if !line.starts_with(".width") {
            continue;
        }

        let width = line
            .strip_prefix(".width")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("line {line_no}: expected `.width <n>`"))?;
        let parsed_width = width
            .parse::<usize>()
            .map_err(|_| format!("line {line_no}: invalid width `{width}`"))?;
        return Ok(Some(parsed_width));
    }

    Ok(None)
}

fn strip_comment(line: &str) -> &str {
    match line.find('#') {
        Some(idx) => &line[..idx],
        None => line,
    }
}
