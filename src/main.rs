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

const DEFAULT_WIDTH: usize = 4;
const SUPPORTED_WIDTHS: &str = "4, 8, 16, 32, 64, 128, 256";

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
    let width = match declared_width(&source) {
        Ok(Some(width)) => width,
        Ok(None) => DEFAULT_WIDTH,
        Err(err) => {
            eprintln!("failed to parse `{path}`: {err}");
            return ExitCode::from(1);
        }
    };

    match width {
        4 => run_for_width::<4>(&path, &source, trace),
        8 => run_for_width::<8>(&path, &source, trace),
        16 => run_for_width::<16>(&path, &source, trace),
        32 => run_for_width::<32>(&path, &source, trace),
        64 => run_for_width::<64>(&path, &source, trace),
        128 => run_for_width::<128>(&path, &source, trace),
        256 => run_for_width::<256>(&path, &source, trace),
        _ => {
            eprintln!("unsupported width {width}; supported widths are: {SUPPORTED_WIDTHS}");
            ExitCode::from(1)
        }
    }
}

fn run_for_width<const W: usize>(path: &str, source: &str, trace: bool) -> ExitCode {
    let program = match parse_program::<W>(source) {
        Ok(program) => program,
        Err(err) => {
            eprintln!("failed to parse `{path}`: {err}");
            return ExitCode::from(1);
        }
    };

    let mut latencies = LatencyTable::default();
    latencies.set(Opcode::Mul, 5);
    let mut cpu = CpuState::<W>::new(latencies);

    if trace {
        let trace = cpu.trace_program(&program);
        print!("{trace}");
        return ExitCode::SUCCESS;
    }

    println!("LWIR VLIW Simulator (W={W})");
    println!("Program: {path}");
    println!("Bundles: {}", program.len());

    while cpu.step(&program) {}
    print_cpu_state(&cpu);
    ExitCode::SUCCESS
}

fn print_usage(exe: &str) {
    eprintln!("usage: {exe} [--trace] <program.lwir>");
    eprintln!("  --trace   emit deterministic per-bundle execution trace");
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
