/// Command-line runner for the LWIR VLIW simulator.
///
/// The binary currently consumes a simple text assembly format. That format is
/// intentionally lightweight so compiler work can start before a binary object
/// format exists.
use lwir_simulator::asm::parse_program;
use lwir_simulator::cpu::{print_cpu_state, CpuState};
use lwir_simulator::isa::Opcode;
use lwir_simulator::latency::LatencyTable;
use std::env;
use std::fs;
use std::process::ExitCode;

const W: usize = 4;

fn main() -> ExitCode {
    let mut args = env::args();
    let exe = args.next().unwrap_or_else(|| "lwir_simulator".to_string());
    let Some(path) = args.next() else {
        eprintln!("usage: {exe} <program.lwir>");
        eprintln!("example:");
        eprintln!("  {exe} examples/hello.lwir");
        return ExitCode::from(2);
    };
    if args.next().is_some() {
        eprintln!("usage: {exe} <program.lwir>");
        return ExitCode::from(2);
    }

    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(err) => {
            eprintln!("failed to read `{path}`: {err}");
            return ExitCode::from(1);
        }
    };
    let program = match parse_program::<W>(&source) {
        Ok(program) => program,
        Err(err) => {
            eprintln!("failed to parse `{path}`: {err}");
            return ExitCode::from(1);
        }
    };

    println!("LWIR VLIW Simulator (W={W})");
    println!("Program: {path}");
    println!("Bundles: {}", program.len());

    let mut latencies = LatencyTable::default();
    latencies.set(Opcode::Mul, 5);
    let mut cpu = CpuState::<W>::new(latencies);

    while cpu.step(&program) {}
    print_cpu_state(&cpu);
    ExitCode::SUCCESS
}
