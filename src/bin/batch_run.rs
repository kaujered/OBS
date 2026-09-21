//! Headless entrypoint for the processing modules.
//!
//! This binary is intentionally thin: it parses CLI arguments, builds the
//! corresponding `Request` and delegates all heavy work to `modes::*`.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Result, anyhow, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use one_big_script_rs_all_in_one::{domain::Mest, modes};

#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Headless runner for native OneBigScript Rust modes."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Ppl(PplArgs),
    Ss(SsArgs),
    Gdi(GdiArgs),
    Telemetry(TelemetryArgs),
    Ved(VedArgs),
}

#[derive(Debug, Clone, Args)]
struct PplArgs {
    #[arg(long)]
    mests: String,
    #[arg(long)]
    year: i32,
    #[arg(long, default_value_t = false)]
    test_mode: bool,
    #[arg(long, default_value_t = false)]
    debug_mode: bool,
    #[arg(long, default_value_t = true)]
    only_last: bool,
}

#[derive(Debug, Clone, Args)]
struct GdiArgs {
    #[arg(long)]
    mests: String,
    #[arg(long)]
    year: i32,
    #[arg(long)]
    month: u32,
    #[arg(long)]
    day: u32,
    #[arg(long, default_value_t = false)]
    test_mode: bool,
    #[arg(long, default_value_t = false)]
    debug_mode: bool,
    #[arg(long, default_value_t = true)]
    only_last: bool,
    #[arg(long, default_value_t = false)]
    include_rejected: bool,
    #[arg(long, value_enum, default_value_t = SourceArg::Dbf)]
    bngkm_source: SourceArg,
    #[arg(long, value_enum, default_value_t = SourceArg::Dbf)]
    hgkm_source: SourceArg,
    #[arg(long, default_value_t = false)]
    export_charts: bool,
}

#[derive(Debug, Clone, Args)]
struct SsArgs {
    #[arg(long)]
    mests: String,
    #[arg(long)]
    year: i32,
    #[arg(long)]
    month: u32,
    #[arg(long, default_value_t = false)]
    test_mode: bool,
    #[arg(long, default_value_t = false)]
    debug_mode: bool,
    #[arg(long, default_value_t = true)]
    only_last: bool,
}

#[derive(Debug, Clone, Args)]
struct VedArgs {
    #[arg(long)]
    mests: String,
    #[arg(long)]
    year: i32,
    #[arg(long)]
    month: u32,
    #[arg(long, default_value_t = false)]
    test_mode: bool,
    #[arg(long, default_value_t = false)]
    debug_mode: bool,
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    correct_work_params: bool,
}

#[derive(Debug, Clone, Args)]
struct TelemetryArgs {
    #[arg(long)]
    year: i32,
    /// Обойти год целиком. По умолчанию выгрузка продолжается с последней
    /// сводки, уже лежащей в папке назначения.
    #[arg(long, default_value_t = false)]
    no_continue_from_last: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SourceArg {
    Dbf,
    Kots,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Ppl(args) => run_ppl(args),
        Command::Ss(args) => run_ss(args),
        Command::Gdi(args) => run_gdi(args),
        Command::Telemetry(args) => run_telemetry(args),
        Command::Ved(args) => run_ved(args),
    }
}

fn run_ppl(args: PplArgs) -> Result<()> {
    let request = modes::ppl::Request {
        mests: parse_mests(&args.mests)?,
        year: args.year,
        test_mode: args.test_mode,
        debug_mode: args.debug_mode,
        only_last: args.only_last,
    };

    let started = Instant::now();
    let output = modes::ppl::execute(&request)?;
    print_summary("ppl", started, &output);
    Ok(())
}

fn run_gdi(args: GdiArgs) -> Result<()> {
    let request = modes::gdi::Request {
        mests: parse_mests(&args.mests)?,
        year: args.year,
        month: args.month,
        day: args.day,
        test_mode: args.test_mode,
        debug_mode: args.debug_mode,
        only_last: args.only_last,
        include_rejected: args.include_rejected,
        bngkm_source: map_source(args.bngkm_source),
        hgkm_source: map_source(args.hgkm_source),
        export_charts: args.export_charts,
        filter_date: None,
    };

    let started = Instant::now();
    let output = modes::gdi::execute(&request)?;
    print_summary("gdi", started, &output);
    Ok(())
}

fn run_ss(args: SsArgs) -> Result<()> {
    let request = modes::ss::Request {
        mests: parse_mests(&args.mests)?,
        year: args.year,
        month: args.month,
        test_mode: args.test_mode,
        debug_mode: args.debug_mode,
        only_last: args.only_last,
    };

    let started = Instant::now();
    let output = modes::ss::execute(&request)?;
    print_summary("ss", started, &output);
    Ok(())
}

fn run_ved(args: VedArgs) -> Result<()> {
    let request = modes::ved::Request {
        mests: parse_mests(&args.mests)?,
        year: args.year,
        month: args.month,
        test_mode: args.test_mode,
        debug_mode: args.debug_mode,
        correct_work_params: args.correct_work_params,
    };

    let started = Instant::now();
    let output = modes::ved::execute(&request)?;
    print_summary("ved", started, &output);
    Ok(())
}

fn run_telemetry(args: TelemetryArgs) -> Result<()> {
    let request = modes::telemetry::Request {
        year: args.year,
        continue_from_last: !args.no_continue_from_last,
    };

    let started = Instant::now();
    let output = modes::telemetry::execute(&request)?;
    print_summary("telemetry", started, &output);
    Ok(())
}

fn map_source(value: SourceArg) -> modes::gdi::DataSourceChoice {
    match value {
        SourceArg::Dbf => modes::gdi::DataSourceChoice::Dbf,
        SourceArg::Kots => modes::gdi::DataSourceChoice::Kots,
    }
}

fn parse_mests(raw: &str) -> Result<Vec<Mest>> {
    let mut parsed = Vec::new();
    for part in raw
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        // Keep aliases close to the business codes so smoke tests are easy to type.
        let mest = match part {
            "1" | "mgpu" => Mest::Mgpu,
            "2" | "yungkm_senoman" | "yungkm" => Mest::YungkmSenoman,
            "3" | "yangkm" => Mest::Yangkm,
            "4" | "bngkm" => Mest::Bngkm,
            "5" | "hgkm" => Mest::Hgkm,
            "7" | "mgpu_nyda" | "nyda" => Mest::MgpuNyda,
            "8" | "yungkm_apt_alb" | "apt_alb" => Mest::YungkmAptAlb,
            _ => return Err(anyhow!("Unknown mest code or alias: {part}")),
        };
        parsed.push(mest);
    }

    if parsed.is_empty() {
        bail!("At least one mest must be provided in --mests");
    }

    Ok(parsed)
}

fn print_summary(mode: &str, started: Instant, output: &[PathBuf]) {
    println!("mode={mode}");
    println!("elapsed_seconds={:.3}", started.elapsed().as_secs_f64());
    println!("output_count={}", output.len());
    for path in output {
        println!("output={}", path.display());
    }
}
