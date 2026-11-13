//! CLI runner for Ordia synthetic generators and benchmark suites.

use anyhow::{Context, Result, bail};
use clap::Parser;
use ordia_bench::{EngineBenchResult, run_engine_scale_in_process};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Parser)]
#[command(name = "ordia-bench", about = "Ordia end-to-end benchmark runner")]
struct Cli {
    /// Benchmark suite to execute (`engine` or `all`).
    #[arg(long, default_value = "engine")]
    suite: String,

    /// Internal worker flag: run a single scale (`10k`, `100k`, `1m`) in an isolated child process and emit JSON.
    #[arg(long)]
    worker_scale: Option<String>,

    /// Force regeneration of synthetic `.xlsx` files under `corpus/generated/`.
    #[arg(long)]
    regenerate: bool,
}

fn workspace_root() -> Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .context("failed to locate workspace root above ordia-bench")
}

fn scale_spec(scale: &str) -> Result<(&'static str, u32, u32, &'static str)> {
    match scale {
        "10k" => Ok(("10k", 10_000, 20, "10k.xlsx")),
        "100k" => Ok(("100k", 100_000, 20, "100k.xlsx")),
        "1m" => Ok(("1m", 1_000_000, 20, "1m.xlsx")),
        other => bail!("unknown benchmark scale `{other}`"),
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let root = workspace_root()?;
    let gen_dir = root.join("corpus/generated");

    if let Some(scale) = &cli.worker_scale {
        let (label, rows, cols, file_name) = scale_spec(scale)?;
        let res = run_engine_scale_in_process(
            label,
            &gen_dir.join(file_name),
            rows,
            cols,
            cli.regenerate,
        )?;
        println!("{}", serde_json::to_string(&res)?);
        return Ok(());
    }

    match cli.suite.as_str() {
        "engine" | "all" => run_engine_suite(cli.regenerate),
        other => bail!("unsupported benchmark suite `{other}` (expected `engine` or `all`)"),
    }
}

fn run_engine_suite(regenerate: bool) -> Result<()> {
    let exe = std::env::current_exe().context("failed to locate ordia-bench executable")?;
    println!(
        "=========================================================================================="
    );
    println!(
        "Ordia Engine Benchmark Suite (20 cols: 70% num, 10% date, 10% CJK/EN text, 10% formulas)"
    );
    println!(
        "=========================================================================================="
    );
    println!(
        "{:<8} | {:>10} | {:>10} | {:>11} | {:>11} | {:>10} | {:>10} | {:>10}",
        "Scale",
        "XLSX Size",
        "Load (ms)",
        "FullRecalc",
        "IncRecalc",
        "Save (ms)",
        "Peak RSS",
        "Net RSS"
    );
    println!(
        "---------+------------+------------+-------------+-------------+------------+------------+-----------"
    );

    let mut results = Vec::new();
    for scale in ["10k", "100k", "1m"] {
        let mut cmd = Command::new(&exe);
        cmd.args(["--worker-scale", scale]);
        if regenerate {
            cmd.arg("--regenerate");
        }
        let out = cmd
            .output()
            .with_context(|| format!("failed to spawn worker for scale {scale}"))?;
        if !out.status.success() {
            bail!(
                "worker for scale `{scale}` failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        let json_line = stdout
            .lines()
            .rfind(|l| l.trim_start().starts_with('{'))
            .context("worker produced no JSON output")?;
        let r: EngineBenchResult = serde_json::from_str(json_line)?;
        println!(
            "{:<8} | {:>7.2} MB | {:>10.1} | {:>8.1} ms | {:>8.2} ms | {:>10.1} | {:>7.1} MB | {:>7.1} MB",
            r.label,
            r.file_size_bytes as f64 / (1024.0 * 1024.0),
            r.load_ms,
            r.full_recalc_ms,
            r.incremental_recalc_ms,
            r.save_ms,
            r.peak_rss_mib,
            r.net_rss_delta_mib,
        );
        results.push(r);
    }
    println!(
        "=========================================================================================="
    );
    Ok(())
}
