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

fn main() -> ExitCode {
    let exe = env::args()
        .next()
        .unwrap_or_else(|| "lwir_simulator".to_string());
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
                eprintln!("usage: {exe} [--trace] <program.lwir>");
                return ExitCode::from(2);
            }
        }
    }

    let Some(path) = path else {
        eprintln!("usage: {exe} [--trace] <program.lwir>");
        eprintln!("example:");
        eprintln!("  {exe} examples/hello.lwir");
        eprintln!("  {exe} --trace examples/hello.lwir");
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

    let mut latencies = LatencyTable::default();
    latencies.set(Opcode::Mul, 5);
    let mut cpu = CpuState::new(program.layout.width, latencies);

    if trace {
        let trace = cpu.trace_program(&program.layout, &program.bundles);
        print!("{trace}");
        return ExitCode::SUCCESS;
    }

    println!("LWIR VLIW Simulator (W={})", program.layout.width);
    println!("Program: {path}");
    println!("Bundles: {}", program.bundles.len());

    while cpu.step(&program.layout, &program.bundles) {}
    print_cpu_state(&cpu);
    ExitCode::SUCCESS
}

fn print_usage(exe: &str) {
    eprintln!("usage: {exe} [--trace] <program.lwir>");
    eprintln!("  --trace   emit deterministic per-bundle execution trace");
}
