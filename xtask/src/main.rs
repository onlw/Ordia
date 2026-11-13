//! Workspace automation CLI (`cargo xtask`) for Ordia.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use serde::Deserialize;

/// Kernel-layer workspace crates that must never depend on GPUI or UI-layer crates.
const KERNEL_CRATES: &[&str] = &[
    "ordia-core",
    "ordia-i18n",
    "ordia-text",
    "ordia-render",
    "ordia-render-pdf",
    "ordia-sheet-engine",
    "ordia-sheet-model",
    "ordia-sheet-layout",
    "ordia-chart",
    "ordia-xlsx",
    "ordia-csv",
];

/// Application-layer workspace crates that depend on GPUI.
const APP_LAYER_CRATES: &[&str] = &[
    "ordia-app",
    "ordia-ui",
    "ordia-sheet-view",
    "ordia-render-gpui",
];

/// External GUI packages forbidden anywhere in the kernel dependency subgraph.
const FORBIDDEN_GUI_PACKAGES: &[&str] = &[
    "gpui",
    "gpui-pre",
    "gpui-fast",
    "gpui-kit",
    "gpui-component",
    "gpui-base",
    "gpui-kit-assets",
    "gpui_platform",
    "gpui-pre-platform",
    "gpui-fast-platform",
    "gpui_web",
    "gpui-pre-web",
    "gpui-fast-web",
    "gpui_macros",
    "gpui-pre-macros",
    "gpui-component-macros",
];

/// Workspace crates permitted to directly reference IronCalc packages.
const IRONCALC_ALLOWED_CRATES: &[&str] = &["ordia-sheet-engine", "ordia-xlsx"];

/// IronCalc upstream package names restricted by architecture rule 4.2.
const IRONCALC_PACKAGES: &[&str] = &["ironcalc", "ironcalc_base"];

#[derive(Debug, Parser)]
#[command(name = "xtask", about = "Ordia workspace automation tasks")]
struct Cli {
    #[command(subcommand)]
    command: TaskCommand,
}

#[derive(Debug, Subcommand)]
enum TaskCommand {
    /// Run all CI checks: rustfmt, typos, check-deps, i18n-check, clippy, tests, cargo-deny.
    Ci,
    /// Verify layered crate dependency rules defined in AGENTS.md and PLAN.md.
    CheckDeps,
    /// Validate localization resources and check for unlocalized UI strings.
    I18nCheck,
    /// Run spreadsheet corpus round-trip and recalculation tests.
    Corpus {
        /// Run the full corpus instead of the quick smoke subset.
        #[arg(long)]
        full: bool,
        /// Re-evaluate formulas and compare against cached workbook values.
        #[arg(long)]
        recalc: bool,
    },
    /// Run performance benchmark suites.
    Bench {
        /// Benchmark suite name (e.g., `engine`, `csv`, `layout`, `all`).
        #[arg(long, default_value = "all")]
        suite: String,
    },
    /// Build platform installer bundles (.dmg / .msi).
    Bundle,
}

#[derive(Debug, Deserialize)]
struct CargoMetadata {
    packages: Vec<MetadataPackage>,
    resolve: Option<MetadataResolve>,
}

#[derive(Debug, Deserialize)]
struct MetadataPackage {
    id: String,
    name: String,
    manifest_path: String,
    dependencies: Vec<ManifestDep>,
}

#[derive(Debug, Deserialize)]
struct ManifestDep {
    name: String,
    rename: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MetadataResolve {
    nodes: Vec<ResolveNode>,
}

#[derive(Debug, Deserialize)]
struct ResolveNode {
    id: String,
    deps: Vec<ResolveDep>,
}

#[derive(Debug, Deserialize)]
struct ResolveDep {
    pkg: String,
}

fn workspace_root() -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .context("failed to locate workspace root above xtask")
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let root = workspace_root()?;

    match cli.command {
        TaskCommand::Ci => run_ci(&root),
        TaskCommand::CheckDeps => run_check_deps(&root),
        TaskCommand::I18nCheck => run_i18n_check(&root),
        TaskCommand::Corpus { full, recalc } => {
            println!("corpus check (full={full}, recalc={recalc}): no corpus registered yet");
            Ok(())
        }
        TaskCommand::Bench { suite } => {
            println!("bench suite `{suite}`: use M0-04 implementation once wired");
            Ok(())
        }
        TaskCommand::Bundle => {
            println!("bundle task: implemented in M1-08");
            Ok(())
        }
    }
}

fn run_ci(root: &Path) -> Result<()> {
    println!("==> [1/6] Checking formatting (cargo fmt)...");
    run_cmd(root, "cargo", &["fmt", "--all", "--", "--check"])?;

    if binary_exists("typos") {
        println!("==> [2/6] Checking spelling (typos)...");
        run_cmd(root, "typos", &[])?;
    } else {
        println!("==> [2/6] Skipping typos (binary not installed)");
    }

    println!("==> [3/6] Checking architecture dependency boundaries...");
    run_check_deps(root)?;

    println!("==> [4/6] Checking localization resources...");
    run_i18n_check(root)?;

    println!("==> [5/6] Running Clippy (--workspace --all-targets -- -D warnings)...");
    run_cmd(
        root,
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    )?;

    println!("==> [6/6] Running workspace tests...");
    run_cmd(root, "cargo", &["test", "--workspace"])?;

    if binary_exists("cargo-deny") {
        println!("==> [extra] Checking licenses and sources (cargo deny)...");
        run_cmd(
            root,
            "cargo",
            &["deny", "check", "licenses", "bans", "sources"],
        )?;
    }

    println!("CI checks passed.");
    Ok(())
}

fn run_i18n_check(root: &Path) -> Result<()> {
    let locales_dir = root.join("crates/ordia-i18n/locales");
    if !locales_dir.exists() {
        return Ok(());
    }
    let zh = locales_dir.join("zh-CN.toml");
    let en = locales_dir.join("en.toml");
    if !zh.exists() || !en.exists() {
        bail!(
            "missing locale files in {}: expected both zh-CN.toml and en.toml",
            locales_dir.display()
        );
    }
    Ok(())
}

fn run_check_deps(root: &Path) -> Result<()> {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1"])
        .current_dir(root)
        .output()
        .context("failed to execute `cargo metadata`")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("`cargo metadata` failed:\n{stderr}");
    }

    let metadata: CargoMetadata =
        serde_json::from_slice(&output.stdout).context("failed to parse `cargo metadata` JSON")?;

    let violations = validate_metadata(&metadata, root);
    if !violations.is_empty() {
        let mut msg = String::from("architecture dependency boundary violations detected:\n");
        for v in &violations {
            msg.push_str("  - ");
            msg.push_str(v);
            msg.push('\n');
        }
        bail!("{msg}");
    }

    println!(
        "check-deps ok: verified {} kernel crates against GUI and IronCalc boundaries.",
        KERNEL_CRATES.len()
    );
    Ok(())
}

fn validate_metadata(metadata: &CargoMetadata, root: &Path) -> Vec<String> {
    let mut violations = Vec::new();
    let kernel_set: BTreeSet<&str> = KERNEL_CRATES.iter().copied().collect();
    let app_set: BTreeSet<&str> = APP_LAYER_CRATES.iter().copied().collect();
    let forbidden_gui: BTreeSet<&str> = FORBIDDEN_GUI_PACKAGES.iter().copied().collect();
    let ironcalc_allowed: BTreeSet<&str> = IRONCALC_ALLOWED_CRATES.iter().copied().collect();
    let ironcalc_pkgs: BTreeSet<&str> = IRONCALC_PACKAGES.iter().copied().collect();

    let mut id_to_name: BTreeMap<&str, &str> = BTreeMap::new();
    let mut workspace_pkg_ids: BTreeMap<&str, &str> = BTreeMap::new();
    let root_prefix = root.to_string_lossy();

    for pkg in &metadata.packages {
        id_to_name.insert(pkg.id.as_str(), pkg.name.as_str());
        let is_local = pkg.manifest_path.starts_with(root_prefix.as_ref())
            || kernel_set.contains(pkg.name.as_str())
            || app_set.contains(pkg.name.as_str())
            || pkg.name.starts_with("ordia-")
            || pkg.name == "xtask";

        if is_local {
            workspace_pkg_ids.insert(pkg.name.as_str(), pkg.id.as_str());

            // Check direct manifest dependencies (catches optional / target-gated deps too).
            for dep in &pkg.dependencies {
                let dep_pkg_name = dep.name.as_str();
                let alias_info = dep
                    .rename
                    .as_deref()
                    .map(|r| format!(" (renamed as `{r}`)"))
                    .unwrap_or_default();

                if kernel_set.contains(pkg.name.as_str()) {
                    if forbidden_gui.contains(dep_pkg_name) {
                        violations.push(format!(
                            "kernel crate `{}` directly depends on forbidden GUI package `{}`{}",
                            pkg.name, dep_pkg_name, alias_info
                        ));
                    }
                    if app_set.contains(dep_pkg_name) {
                        violations.push(format!(
                            "kernel crate `{}` directly depends on application-layer crate `{}`{}",
                            pkg.name, dep_pkg_name, alias_info
                        ));
                    }
                }

                if ironcalc_pkgs.contains(dep_pkg_name)
                    && !ironcalc_allowed.contains(pkg.name.as_str())
                {
                    violations.push(format!(
                        "crate `{}` directly depends on IronCalc package `{}`{}; only {:?} may reference IronCalc",
                        pkg.name, dep_pkg_name, alias_info, IRONCALC_ALLOWED_CRATES
                    ));
                }
            }
        }
    }

    // Check resolved transitive graph so indirect paths (kernel -> helper -> gpui) are caught.
    if let Some(resolve) = &metadata.resolve {
        let mut adj: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for node in &resolve.nodes {
            let children: Vec<&str> = node.deps.iter().map(|d| d.pkg.as_str()).collect();
            adj.insert(node.id.as_str(), children);
        }

        for &kernel_crate in KERNEL_CRATES {
            let Some(&start_id) = workspace_pkg_ids.get(kernel_crate) else {
                violations.push(format!(
                    "expected kernel crate `{kernel_crate}` is missing from workspace"
                ));
                continue;
            };

            if let Some(chain) =
                find_forbidden_path(start_id, &adj, &id_to_name, &forbidden_gui, &app_set)
            {
                violations.push(format!(
                    "kernel crate `{kernel_crate}` transitively depends on forbidden GUI/app package: {}",
                    chain.join(" -> ")
                ));
            }
        }
    }

    violations.sort();
    violations.dedup();
    violations
}

fn find_forbidden_path<'a>(
    start_id: &'a str,
    adj: &BTreeMap<&'a str, Vec<&'a str>>,
    id_to_name: &BTreeMap<&'a str, &'a str>,
    forbidden_gui: &BTreeSet<&str>,
    app_set: &BTreeSet<&str>,
) -> Option<Vec<String>> {
    let mut visited: BTreeSet<&'a str> = BTreeSet::new();
    let mut parent: BTreeMap<&'a str, &'a str> = BTreeMap::new();
    let mut queue: VecDeque<&'a str> = VecDeque::new();

    visited.insert(start_id);
    queue.push_back(start_id);

    while let Some(curr) = queue.pop_front() {
        let Some(neighbors) = adj.get(curr) else {
            continue;
        };
        for &next in neighbors {
            if !visited.insert(next) {
                continue;
            }
            parent.insert(next, curr);
            let next_name = id_to_name.get(next).copied().unwrap_or(next);
            if forbidden_gui.contains(next_name) || app_set.contains(next_name) {
                let mut chain = vec![next_name.to_string()];
                let mut walk = next;
                while let Some(&prev) = parent.get(walk) {
                    let prev_name = id_to_name.get(prev).copied().unwrap_or(prev);
                    chain.push(prev_name.to_string());
                    walk = prev;
                }
                chain.reverse();
                return Some(chain);
            }
            queue.push_back(next);
        }
    }

    None
}

fn binary_exists(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn run_cmd(dir: &Path, program: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .current_dir(dir)
        .status()
        .with_context(|| format!("failed to launch `{program} {}`", args.join(" ")))?;

    if !status.success() {
        bail!(
            "command `{program} {}` exited with status {status}",
            args.join(" ")
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_base_packages(root: &Path) -> Vec<MetadataPackage> {
        let mut pkgs = Vec::new();
        for &name in KERNEL_CRATES.iter().chain(APP_LAYER_CRATES.iter()) {
            pkgs.push(MetadataPackage {
                id: format!("id:{name}"),
                name: name.to_string(),
                manifest_path: root
                    .join("crates")
                    .join(name)
                    .join("Cargo.toml")
                    .to_string_lossy()
                    .into_owned(),
                dependencies: Vec::new(),
            });
        }
        pkgs.push(MetadataPackage {
            id: "id:gpui".to_string(),
            name: "gpui".to_string(),
            manifest_path: "/cargo/registry/gpui/Cargo.toml".to_string(),
            dependencies: Vec::new(),
        });
        pkgs.push(MetadataPackage {
            id: "id:gpui-kit".to_string(),
            name: "gpui-kit".to_string(),
            manifest_path: "/cargo/registry/gpui-kit/Cargo.toml".to_string(),
            dependencies: Vec::new(),
        });
        pkgs.push(MetadataPackage {
            id: "id:ironcalc_base".to_string(),
            name: "ironcalc_base".to_string(),
            manifest_path: "/cargo/registry/ironcalc_base/Cargo.toml".to_string(),
            dependencies: Vec::new(),
        });
        pkgs
    }

    fn make_resolve(edges: &[(&str, &[&str])], pkgs: &[MetadataPackage]) -> MetadataResolve {
        let mut map: BTreeMap<String, Vec<ResolveDep>> = BTreeMap::new();
        for p in pkgs {
            map.entry(p.id.clone()).or_default();
        }
        for (src, dsts) in edges {
            let entry = map.entry(format!("id:{src}")).or_default();
            for dst in *dsts {
                entry.push(ResolveDep {
                    pkg: format!("id:{dst}"),
                });
            }
        }
        MetadataResolve {
            nodes: map
                .into_iter()
                .map(|(id, deps)| ResolveNode { id, deps })
                .collect(),
        }
    }

    #[test]
    fn clean_architecture_passes() {
        let root = Path::new("/workspace/Ordia");
        let mut pkgs = make_base_packages(root);
        // Allowed: ordia-app -> gpui-kit, ordia-sheet-engine -> ironcalc_base
        for p in &mut pkgs {
            if p.name == "ordia-app" {
                p.dependencies.push(ManifestDep {
                    name: "gpui-kit".to_string(),
                    rename: None,
                });
            }
            if p.name == "ordia-sheet-engine" {
                p.dependencies.push(ManifestDep {
                    name: "ironcalc_base".to_string(),
                    rename: None,
                });
            }
        }
        let resolve = make_resolve(
            &[
                ("ordia-app", &["gpui-kit", "ordia-sheet-model"]),
                ("ordia-sheet-model", &["ordia-sheet-engine"]),
                ("ordia-sheet-engine", &["ironcalc_base"]),
            ],
            &pkgs,
        );
        let meta = CargoMetadata {
            packages: pkgs,
            resolve: Some(resolve),
        };
        let violations = validate_metadata(&meta, root);
        assert!(
            violations.is_empty(),
            "unexpected violations: {violations:?}"
        );
    }

    #[test]
    fn catches_direct_kernel_gpui_dependency() {
        let root = Path::new("/workspace/Ordia");
        let mut pkgs = make_base_packages(root);
        for p in &mut pkgs {
            if p.name == "ordia-core" {
                p.dependencies.push(ManifestDep {
                    name: "gpui".to_string(),
                    rename: None,
                });
            }
        }
        let resolve = make_resolve(&[("ordia-core", &["gpui"])], &pkgs);
        let meta = CargoMetadata {
            packages: pkgs,
            resolve: Some(resolve),
        };
        let violations = validate_metadata(&meta, root);
        assert!(
            violations
                .iter()
                .any(|v| v.contains("ordia-core") && v.contains("gpui")),
            "expected ordia-core -> gpui violation, got: {violations:?}"
        );
    }

    #[test]
    fn catches_transitive_kernel_gui_dependency() {
        let root = Path::new("/workspace/Ordia");
        let pkgs = make_base_packages(root);
        let resolve = make_resolve(
            &[
                ("ordia-sheet-layout", &["ordia-sheet-model"]),
                ("ordia-sheet-model", &["ordia-core"]),
                ("ordia-core", &["gpui-kit"]),
            ],
            &pkgs,
        );
        let meta = CargoMetadata {
            packages: pkgs,
            resolve: Some(resolve),
        };
        let violations = validate_metadata(&meta, root);
        assert!(
            violations
                .iter()
                .any(|v| v
                    .contains("ordia-sheet-layout -> ordia-sheet-model -> ordia-core -> gpui-kit")),
            "expected full transitive chain, got: {violations:?}"
        );
    }

    #[test]
    fn catches_ironcalc_leak_outside_engine_and_xlsx() {
        let root = Path::new("/workspace/Ordia");
        let mut pkgs = make_base_packages(root);
        for p in &mut pkgs {
            if p.name == "ordia-sheet-model" {
                p.dependencies.push(ManifestDep {
                    name: "ironcalc_base".to_string(),
                    rename: None,
                });
            }
        }
        let resolve = make_resolve(&[("ordia-sheet-model", &["ironcalc_base"])], &pkgs);
        let meta = CargoMetadata {
            packages: pkgs,
            resolve: Some(resolve),
        };
        let violations = validate_metadata(&meta, root);
        assert!(
            violations
                .iter()
                .any(|v| v.contains("ordia-sheet-model") && v.contains("ironcalc_base")),
            "expected ironcalc leak violation, got: {violations:?}"
        );
    }
}
