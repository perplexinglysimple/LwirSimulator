// Command-line runner for the VLIW simulator.
//
// The binary currently consumes a simple text assembly format. That format is
// intentionally lightweight so compiler work can start before a binary object
// format exists.
use std::env;
use std::fs;
use std::process::ExitCode;
use vliw_simulator::asm::parse_program;
use vliw_simulator::cpu::{print_cpu_state, CpuState};
use vliw_simulator::isa::Opcode;
use vliw_simulator::latency::LatencyTable;
use vliw_simulator::system::System;

fn main() -> ExitCode {
    let exe = env::args()
        .next()
        .unwrap_or_else(|| "vliw_simulator".to_string());
    let mut trace = false;
    let mut path = None::<String>;

    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--trace" => trace = true,
            "-h" | "--help" => {
                print_usage(&exe);
                return ExitCode::SUCCESS;
            }
            _ if path.is_none() => path = Some(arg),
            _ => {
                eprintln!("usage: {exe} [--trace] <program.vliw>");
                return ExitCode::from(2);
            }
        }
    }

    let Some(path) = path else {
        eprintln!("usage: {exe} [--trace] <program.vliw>");
        eprintln!("example:");
        eprintln!("  {exe} examples/hello.vliw");
        eprintln!("  {exe} --trace examples/hello.vliw");
        return ExitCode::from(2);
    };

    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(err) => {
            eprintln!("failed to read `{path}`: {err}");
            return ExitCode::from(1);
        }
    };
    let program = match parse_program(&source) {
        Ok(program) => program,
        Err(err) => {
            eprintln!("failed to parse `{path}`: {err}");
            return ExitCode::from(1);
        }
    };

    if program.layout.topology.cpus != 1 {
        eprintln!(
            "failed to run `{path}`: topology declares {} CPUs, but the CLI accepts one program file",
            program.layout.topology.cpus
        );
        return ExitCode::from(1);
    }

    if trace {
        let mut latencies = LatencyTable::default();
        latencies.set(Opcode::Mul, 5);
        let mut cpu = CpuState::new_for_layout(&program.layout, latencies);
        let trace = cpu.trace_program(&program.layout, &program.bundles);
        print!("{trace}");
        return ExitCode::SUCCESS;
    }

    println!("VLIW Simulator (W={})", program.layout.width);
    println!("Program: {path}");
    println!("Bundles: {}", program.bundles.len());

    let mut latencies = LatencyTable::default();
    latencies.set(Opcode::Mul, 5);
    let mut system = match System::from_program(program, latencies) {
        Ok(system) => system,
        Err(err) => {
            eprintln!("failed to initialize system for `{path}`: {err}");
            return ExitCode::from(1);
        }
    };
    system.run_until_quiescent();
    print_cpu_state(&system.cpus[0]);
    ExitCode::SUCCESS
}

fn print_usage(exe: &str) {
    eprintln!("usage: {exe} [--trace] <program.vliw>");
    eprintln!("  --trace   emit deterministic per-bundle execution trace");
}
