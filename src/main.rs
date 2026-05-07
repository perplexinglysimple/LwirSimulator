// Command-line runner for the VLIW simulator.
//
// The binary currently consumes a simple text assembly format. That format is
// intentionally lightweight so compiler work can start before a binary object
// format exists.
use std::env;
use std::fmt::Write;
use std::fs;
use std::panic::{catch_unwind, set_hook, take_hook, AssertUnwindSafe};
use std::process::ExitCode;
use vliw_simulator::asm::parse_program;
use vliw_simulator::cache::CacheOutcome;
use vliw_simulator::cpu::TraceMemoryEffect;
use vliw_simulator::cpu::{print_cpu_state, CpuState};
use vliw_simulator::latency::LatencyTable;
use vliw_simulator::system::System;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputMode {
    Human,
    TraceText,
    Json,
    Dump,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DumpMemQuery {
    address: usize,
    width_bytes: usize,
}

fn main() -> ExitCode {
    let exe = env::args()
        .next()
        .unwrap_or_else(|| "vliw_simulator".to_string());
    let mut output_mode = OutputMode::Human;
    let mut dump_regs = Vec::<usize>::new();
    let mut dump_mems = Vec::<DumpMemQuery>::new();
    let mut dump_all_regs = false;
    let mut path = None::<String>;

    let args: Vec<String> = env::args().skip(1).collect();
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--trace" => {
                if output_mode == OutputMode::Dump {
                    eprintln!("--trace cannot be combined with dump options");
                    return ExitCode::from(2);
                }
                output_mode = OutputMode::TraceText;
            }
            "--json" | "--trace=json" => {
                if output_mode == OutputMode::Dump {
                    eprintln!("--json cannot be combined with dump options");
                    return ExitCode::from(2);
                }
                output_mode = OutputMode::Json;
            }
            "--dump-reg" => {
                if output_mode != OutputMode::Human && output_mode != OutputMode::Dump {
                    eprintln!("--dump-reg cannot be combined with --trace or --json");
                    return ExitCode::from(2);
                }
                let Some(reg) = args.get(i + 1) else {
                    eprintln!("--dump-reg requires a register such as r1");
                    return ExitCode::from(2);
                };
                let Some(reg) = parse_reg(reg) else {
                    eprintln!("invalid register `{reg}`; expected rN");
                    return ExitCode::from(2);
                };
                output_mode = OutputMode::Dump;
                dump_regs.push(reg);
                i += 1;
            }
            "--dump-mem" => {
                if output_mode != OutputMode::Human && output_mode != OutputMode::Dump {
                    eprintln!("--dump-mem cannot be combined with --trace or --json");
                    return ExitCode::from(2);
                }
                let Some(query) = args.get(i + 1) else {
                    eprintln!("--dump-mem requires an address and width such as 0x100:4");
                    return ExitCode::from(2);
                };
                let Some(query) = parse_mem_query(query) else {
                    eprintln!("invalid memory query `{query}`; expected addr:width with width 1, 2, 4, or 8");
                    return ExitCode::from(2);
                };
                output_mode = OutputMode::Dump;
                dump_mems.push(query);
                i += 1;
            }
            "--dump-all-regs" => {
                if output_mode != OutputMode::Human && output_mode != OutputMode::Dump {
                    eprintln!("--dump-all-regs cannot be combined with --trace or --json");
                    return ExitCode::from(2);
                }
                output_mode = OutputMode::Dump;
                dump_all_regs = true;
            }
            "-h" | "--help" => {
                print_usage(&exe);
                return ExitCode::SUCCESS;
            }
            _ if path.is_none() => path = Some(args[i].clone()),
            _ => {
                eprintln!(
                    "usage: {exe} [--trace|--json|--trace=json|--dump-reg rN|--dump-mem addr:width|--dump-all-regs] <program.vliw>"
                );
                return ExitCode::from(2);
            }
        }
        i += 1;
    }

    let Some(path) = path else {
        eprintln!(
            "usage: {exe} [--trace|--json|--trace=json|--dump-reg rN|--dump-mem addr:width|--dump-all-regs] <program.vliw>"
        );
        eprintln!("example:");
        eprintln!("  {exe} examples/hello.vliw");
        eprintln!("  {exe} --trace examples/hello.vliw");
        eprintln!("  {exe} --json examples/hello.vliw");
        eprintln!("  {exe} --dump-reg r1 --dump-mem 0x100:4 examples/hello.vliw");
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

    if output_mode == OutputMode::TraceText {
        let mut cpu = CpuState::new_for_layout(&program.layout, LatencyTable::default());
        let trace = match catch_simulation(|| cpu.trace_program(&program.layout, &program.bundles))
        {
            Ok(trace) => trace,
            Err(err) => return memory_error_exit(err),
        };
        print!("{trace}");
        return ExitCode::SUCCESS;
    }

    if output_mode == OutputMode::Json {
        let mut cpu = CpuState::new_for_layout(&program.layout, LatencyTable::default());
        let trace = match catch_simulation(|| cpu.trace_program(&program.layout, &program.bundles))
        {
            Ok(trace) => trace,
            Err(err) => return memory_error_exit(err),
        };
        print!("{}", final_state_json(&cpu, &trace));
        return ExitCode::SUCCESS;
    }

    if output_mode == OutputMode::Dump {
        let mut system = match System::from_program(program, LatencyTable::default()) {
            Ok(system) => system,
            Err(err) => {
                eprintln!("failed to initialize system for `{path}`: {err}");
                return ExitCode::from(1);
            }
        };
        if let Err(err) = catch_simulation(|| system.run_until_quiescent()) {
            return memory_error_exit(err);
        }
        print_dump_output(&system.cpus[0], dump_all_regs, &dump_regs, &dump_mems);
        return ExitCode::SUCCESS;
    }

    println!("VLIW Simulator (W={})", program.layout.width);
    println!("Program: {path}");
    println!("Bundles: {}", program.bundles.len());

    let mut system = match System::from_program(program, LatencyTable::default()) {
        Ok(system) => system,
        Err(err) => {
            eprintln!("failed to initialize system for `{path}`: {err}");
            return ExitCode::from(1);
        }
    };
    if let Err(err) = catch_simulation(|| system.run_until_quiescent()) {
        return memory_error_exit(err);
    }
    print_cpu_state(&system.cpus[0]);
    ExitCode::SUCCESS
}

fn catch_simulation<T>(f: impl FnOnce() -> T) -> Result<T, Box<dyn std::any::Any + Send>> {
    let hook = take_hook();
    set_hook(Box::new(|_| {}));
    let result = catch_unwind(AssertUnwindSafe(f));
    set_hook(hook);
    result
}

fn memory_error_exit(err: Box<dyn std::any::Any + Send>) -> ExitCode {
    if let Some(message) = err.downcast_ref::<String>() {
        eprintln!("{message}");
    } else if let Some(message) = err.downcast_ref::<&str>() {
        eprintln!("{message}");
    } else {
        eprintln!("error: simulation failed");
    }
    ExitCode::from(1)
}

fn print_usage(exe: &str) {
    eprintln!(
        "usage: {exe} [--trace|--json|--trace=json|--dump-reg rN|--dump-mem addr:width|--dump-all-regs] <program.vliw>"
    );
    eprintln!("  --trace          emit deterministic per-bundle execution trace");
    eprintln!("  --json           emit final architectural state as JSON");
    eprintln!("  --trace=json     alias for --json");
    eprintln!("  --dump-reg rN    emit one final GPR value, including zero");
    eprintln!("  --dump-mem A:W   emit one final little-endian memory value");
    eprintln!("  --dump-all-regs  emit all final GPR values, including zeros");
}

fn parse_reg(reg: &str) -> Option<usize> {
    let number = reg.strip_prefix('r')?;
    number.parse().ok()
}

fn parse_mem_query(query: &str) -> Option<DumpMemQuery> {
    let (address, width_bytes) = query.split_once(':')?;
    let address = parse_usize_literal(address)?;
    let width_bytes = parse_usize_literal(width_bytes)?;
    if !matches!(width_bytes, 1 | 2 | 4 | 8) {
        return None;
    }
    Some(DumpMemQuery {
        address,
        width_bytes,
    })
}

fn parse_usize_literal(value: &str) -> Option<usize> {
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        usize::from_str_radix(hex, 16).ok()
    } else {
        value.parse().ok()
    }
}

fn print_dump_output(
    cpu: &CpuState,
    dump_all_regs: bool,
    dump_regs: &[usize],
    dump_mems: &[DumpMemQuery],
) {
    if dump_all_regs {
        for reg in 0..cpu.num_gprs {
            print_dump_reg(cpu, reg);
        }
    }
    for reg in dump_regs {
        print_dump_reg(cpu, *reg);
    }
    for query in dump_mems {
        print_dump_mem(cpu, *query);
    }
}

fn print_dump_reg(cpu: &CpuState, reg: usize) {
    let value = if reg < cpu.gprs.len() {
        cpu.gprs[reg]
    } else {
        0
    };
    println!("reg r{} value={:#018x} ({})", reg, value, value);
}

fn print_dump_mem(cpu: &CpuState, query: DumpMemQuery) {
    let in_bounds = query.width_bytes <= cpu.memory.len()
        && query.address <= cpu.memory.len().saturating_sub(query.width_bytes);
    let mut value = 0u64;
    if in_bounds {
        for offset in 0..query.width_bytes {
            value |= (cpu.memory[query.address + offset] as u64) << (offset * 8);
        }
    }
    println!(
        "mem addr={:#010x} width={} value=0x{:0width$x} ({}) in_bounds={}",
        query.address,
        query.width_bytes,
        value,
        value,
        in_bounds,
        width = query.width_bytes * 2
    );
}

fn final_state_json(cpu: &CpuState, trace: &vliw_simulator::cpu::TraceLog) -> String {
    let mut out = String::new();

    writeln!(&mut out, "{{").unwrap();
    writeln!(&mut out, "  \"format\": \"vliw-sim-final-state-v1\",").unwrap();
    writeln!(&mut out, "  \"halted\": {},", cpu.halted).unwrap();
    writeln!(&mut out, "  \"pc\": {},", cpu.pc).unwrap();
    writeln!(&mut out, "  \"cycle\": {},", cpu.cycle).unwrap();
    writeln!(&mut out, "  \"registers\": {{").unwrap();
    for (i, value) in cpu.gprs.iter().enumerate() {
        let comma = if i + 1 == cpu.gprs.len() { "" } else { "," };
        writeln!(&mut out, "    \"r{}\": {}{}", i, value, comma).unwrap();
    }
    writeln!(&mut out, "  }},").unwrap();
    writeln!(&mut out, "  \"predicates\": {{").unwrap();
    for (i, value) in cpu.preds.iter().enumerate() {
        let comma = if i + 1 == cpu.preds.len() { "" } else { "," };
        writeln!(&mut out, "    \"p{}\": {}{}", i, value, comma).unwrap();
    }
    writeln!(&mut out, "  }},").unwrap();
    writeln!(&mut out, "  \"memory_writes\": [").unwrap();

    let stores: Vec<_> = trace
        .events
        .iter()
        .flat_map(|event| event.memory_effects.iter())
        .filter_map(|effect| match effect {
            TraceMemoryEffect::Store {
                width_bytes,
                address,
                value,
                in_bounds,
                cache_outcome,
                ..
            } => Some((*address, *width_bytes, *value, *in_bounds, *cache_outcome)),
            TraceMemoryEffect::Load { .. } => None,
        })
        .collect();

    for (i, (address, width_bytes, value, in_bounds, cache_outcome)) in stores.iter().enumerate() {
        let comma = if i + 1 == stores.len() { "" } else { "," };
        writeln!(
            &mut out,
            "    {{ \"addr\": {}, \"width\": {}, \"value\": {}, \"in_bounds\": {}, \"cache\": \"{}\" }}{}",
            address,
            width_bytes,
            value,
            in_bounds,
            json_cache_outcome(*cache_outcome),
            comma
        )
        .unwrap();
    }
    writeln!(&mut out, "  ]").unwrap();
    writeln!(&mut out, "}}").unwrap();

    out
}

fn json_cache_outcome(outcome: CacheOutcome) -> &'static str {
    match outcome {
        CacheOutcome::Hit => "hit",
        CacheOutcome::Miss => "miss",
        CacheOutcome::MissDirty => "miss_dirty",
    }
}
