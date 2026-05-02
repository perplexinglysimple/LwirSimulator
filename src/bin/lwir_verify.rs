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

    let program = match parse_program(&source) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: parse failed for `{path}`: {e}");
            return ExitCode::from(2);
        }
    };

    let latencies = LatencyTable::default();
    let diags = verify_program(&program.layout, &program.bundles, &latencies);

    println!("LWIR Verifier (W={})", program.layout.width);
    println!("Program : {path}");
    println!("Bundles : {}", program.bundles.len());

    if diags.is_empty() {
        println!("Result  : CLEAN — no contract violations found");
        return ExitCode::SUCCESS;
    }

    println!("Result  : {} violation(s) found", diags.len());
    println!();
    for d in &diags {
        let rule_tag = match d.rule {
            Rule::SlotOpcodeLegality => "slot-opcode-legality",
            Rule::SameBundleGprRaw => "same-bundle-gpr-raw",
            Rule::SameBundleGprWaw => "same-bundle-gpr-waw",
            Rule::SameBundlePredHazard => "same-bundle-pred-hazard",
            Rule::GprReadyCycle => "gpr-ready-cycle",
        };
        println!("[{rule_tag}] {}", d.message);
    }

    ExitCode::from(1)
}
