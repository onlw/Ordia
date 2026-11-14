//! Platform installer bundler (`.dmg` on macOS, `.wxs`/`.msi` on Windows) and automated
//! third-party open-source license manifest generator (`M1-08`, `ADR 0001`).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct FullMetadata {
    packages: Vec<LicensePackage>,
    workspace_members: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct LicensePackage {
    id: String,
    name: String,
    version: String,
    license: Option<String>,
    license_file: Option<String>,
    repository: Option<String>,
}

/// Summary row for a single third-party crate dependency.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThirdPartyCrateEntry {
    /// Crate name.
    pub name: String,
    /// SemVer string.
    pub version: String,
    /// SPDX license expression.
    pub license: String,
    /// Upstream repository URL, if declared.
    pub repository: String,
}

/// Generates `assets/THIRD_PARTY_LICENSES.txt` directly from `cargo metadata` and returns `(output_path, count)`.
pub fn generate_third_party_licenses(root: &Path) -> Result<(PathBuf, usize)> {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1"])
        .current_dir(root)
        .output()
        .context("failed to execute `cargo metadata` for license generation")?;
    if !output.status.success() {
        bail!(
            "`cargo metadata` failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let meta: FullMetadata =
        serde_json::from_slice(&output.stdout).context("failed to parse `cargo metadata` JSON")?;
    let ws_set: std::collections::BTreeSet<&str> =
        meta.workspace_members.iter().map(String::as_str).collect();

    let mut dedup: BTreeMap<(String, String), ThirdPartyCrateEntry> = BTreeMap::new();
    for pkg in meta.packages {
        if ws_set.contains(pkg.id.as_str()) {
            continue;
        }
        let license = pkg
            .license
            .or_else(|| pkg.license_file.map(|f| format!("LicenseFile({f})")))
            .unwrap_or_default();
        if license.trim().is_empty() {
            bail!(
                "third-party crate `{} v{}` is missing a declared SPDX license",
                pkg.name,
                pkg.version
            );
        }
        dedup.insert(
            (pkg.name.clone(), pkg.version.clone()),
            ThirdPartyCrateEntry {
                name: pkg.name,
                version: pkg.version,
                license,
                repository: pkg.repository.unwrap_or_else(|| "-".to_string()),
            },
        );
    }

    let entries: Vec<ThirdPartyCrateEntry> = dedup.into_values().collect();
    let rendered = format_third_party_licenses_report(&entries);
    let out_path = root.join("assets").join("THIRD_PARTY_LICENSES.txt");
    if let Some(parent) = out_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&out_path, rendered)
        .with_context(|| format!("failed to write {}", out_path.display()))?;
    Ok((out_path, entries.len()))
}

/// Formats a deterministic plain-text third-party notices report.
pub fn format_third_party_licenses_report(entries: &[ThirdPartyCrateEntry]) -> String {
    let mut out = String::new();
    out.push_str("# Ordia Third-Party Open Source Notices\n");
    out.push_str("# Generated automatically by `cargo xtask bundle` from `cargo metadata`.\n");
    out.push_str("# Format: crate | version | spdx_license | repository\n\n");
    for e in entries {
        out.push_str(&format!(
            "{} | {} | {} | {}\n",
            e.name, e.version, e.license, e.repository
        ));
    }
    out
}

/// Renders a WiX v3 XML installer definition (`Ordia.wxs`) for Windows `.msi` packaging.
pub fn render_windows_wix_manifest(
    version: &str,
    exe_path: &Path,
    ico_path: &Path,
    licenses_path: &Path,
) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Wix xmlns="http://schemas.microsoft.com/wix/2006/wi">
  <Product Id="*"
           Name="Ordia"
           Language="1033"
           Version="{version}"
           Manufacturer="Ordia Contributors"
           UpgradeCode="7B2C8E14-5D4F-4E2A-9B01-6A9D3F2184C0">
    <Package InstallerVersion="500" Compressed="yes" InstallScope="perMachine" Platform="x64"/>
    <MajorUpgrade DowngradeErrorMessage="A newer version of Ordia is already installed."/>
    <MediaTemplate EmbedCab="yes"/>
    <Icon Id="OrdiaIcon" SourceFile="{ico}"/>
    <Property Id="ARPPRODUCTICON" Value="OrdiaIcon"/>

    <Directory Id="TARGETDIR" Name="SourceDir">
      <Directory Id="ProgramFiles64Folder">
        <Directory Id="INSTALLFOLDER" Name="Ordia"/>
      </Directory>
    </Directory>

    <ComponentGroup Id="ProductComponents" Directory="INSTALLFOLDER">
      <Component Id="MainExecutable" Guid="3F5A9E71-98C2-4D3B-A1E0-88C41D92B701" Win64="yes">
        <File Id="OrdiaExe" Name="ordia-app.exe" Source="{exe}" KeyPath="yes"/>
        <File Id="ThirdPartyLicenses" Name="THIRD_PARTY_LICENSES.txt" Source="{lic}"/>
        <ProgId Id="Ordia.Spreadsheet.xlsx" Description="Ordia OpenXML Spreadsheet (.xlsx)" Icon="OrdiaExe">
          <Extension Id="xlsx" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet">
            <Verb Id="open" Command="Open with Ordia" TargetFile="OrdiaExe" Argument="&quot;%1&quot;"/>
          </Extension>
        </ProgId>
        <ProgId Id="Ordia.Spreadsheet.csv" Description="Ordia Comma-Separated Values (.csv)" Icon="OrdiaExe">
          <Extension Id="csv" ContentType="text/csv">
            <Verb Id="open" Command="Open with Ordia" TargetFile="OrdiaExe" Argument="&quot;%1&quot;"/>
          </Extension>
        </ProgId>
      </Component>
    </ComponentGroup>

    <Feature Id="ProductFeature" Title="Ordia" Level="1">
      <ComponentGroupRef Id="ProductComponents"/>
    </Feature>
  </Product>
</Wix>
"#,
        exe = exe_path.display(),
        ico = ico_path.display(),
        lic = licenses_path.display(),
    )
}

/// Executes the platform bundle pipeline (`cargo xtask bundle`).
pub fn run_bundle(root: &Path, licenses_only: bool, skip_build: bool) -> Result<()> {
    let (licenses_path, count) = generate_third_party_licenses(root)?;
    println!(
        "bundle: generated {} with {count} third-party crate notices.",
        licenses_path.display()
    );
    if licenses_only {
        return Ok(());
    }

    if !skip_build {
        println!("bundle: building release binary `ordia-app`...");
        let status = Command::new("cargo")
            .args(["build", "--release", "-p", "ordia-app"])
            .current_dir(root)
            .status()
            .context("failed to invoke `cargo build --release -p ordia-app`")?;
        if !status.success() {
            bail!("`cargo build --release -p ordia-app` failed");
        }
    }

    let version = env!("CARGO_PKG_VERSION");
    let arch = std::env::consts::ARCH;
    let bundle_root = root.join("target").join("bundle");
    fs::create_dir_all(&bundle_root)?;

    // Always render the Windows WiX manifest so cross-platform CI can inspect/validate it.
    let win_dir = bundle_root.join("windows");
    fs::create_dir_all(&win_dir)?;
    let win_exe = root.join("target").join("release").join("ordia-app.exe");
    let ico_path = root.join("assets").join("icons").join("Ordia.ico");
    let wxs_path = win_dir.join("Ordia.wxs");
    let wxs_xml = render_windows_wix_manifest(version, &win_exe, &ico_path, &licenses_path);
    fs::write(&wxs_path, wxs_xml)?;

    if cfg!(target_os = "macos") {
        let dmg_path = build_macos_dmg(root, &bundle_root, &licenses_path, version, arch)?;
        println!(
            "bundle ok: macOS disk image created at {}",
            dmg_path.display()
        );
    } else if cfg!(target_os = "windows") {
        let msi_path = build_windows_msi(root, &bundle_root, &wxs_path, version, arch)?;
        println!("bundle ok: Windows MSI created at {}", msi_path.display());
    } else {
        println!(
            "bundle ok: generated licenses and WiX manifest at {} (host OS has no DMG/MSI packager)",
            wxs_path.display()
        );
    }
    Ok(())
}

fn build_macos_dmg(
    root: &Path,
    bundle_root: &Path,
    licenses_path: &Path,
    version: &str,
    arch: &str,
) -> Result<PathBuf> {
    let staging = bundle_root.join("macos").join("staging");
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    let contents = staging.join("Ordia.app").join("Contents");
    let macos_bin_dir = contents.join("MacOS");
    let resources_dir = contents.join("Resources");
    fs::create_dir_all(&macos_bin_dir)?;
    fs::create_dir_all(&resources_dir)?;

    let bin_src = root.join("target").join("release").join("ordia-app");
    if !bin_src.exists() {
        bail!("release binary not found at {}", bin_src.display());
    }
    let bin_dst = macos_bin_dir.join("ordia-app");
    fs::copy(&bin_src, &bin_dst)?;

    let plist_src = fs::read_to_string(root.join("assets").join("macos").join("Info.plist"))?;
    let plist_rendered = plist_src.replace(
        "<string>0.1.0</string>",
        &format!("<string>{version}</string>"),
    );
    fs::write(contents.join("Info.plist"), plist_rendered)?;

    fs::copy(
        root.join("assets").join("icons").join("Ordia.icns"),
        resources_dir.join("Ordia.icns"),
    )?;
    fs::copy(
        licenses_path,
        resources_dir.join("THIRD_PARTY_LICENSES.txt"),
    )?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let _ = symlink("/Applications", staging.join("Applications"));
    }

    // Optional codesign hook for M5-01 when APPLE_SIGNING_IDENTITY is configured.
    if let Ok(identity) = std::env::var("APPLE_SIGNING_IDENTITY")
        && !identity.trim().is_empty()
    {
        let status = Command::new("codesign")
            .args([
                "--force",
                "--options",
                "runtime",
                "--deep",
                "--sign",
                identity.trim(),
            ])
            .arg(staging.join("Ordia.app"))
            .status()
            .context("failed to invoke `codesign`")?;
        if !status.success() {
            bail!("`codesign` failed");
        }
    }

    let dmg_out = bundle_root.join(format!("Ordia-{version}-macos-{arch}.dmg"));
    let status = Command::new("hdiutil")
        .args(["create", "-volname", "Ordia", "-srcfolder"])
        .arg(&staging)
        .args(["-ov", "-format", "UDZO"])
        .arg(&dmg_out)
        .status()
        .context("failed to execute `hdiutil create`")?;
    if !status.success() {
        bail!("`hdiutil create` failed");
    }
    Ok(dmg_out)
}

fn build_windows_msi(
    root: &Path,
    bundle_root: &Path,
    wxs_path: &Path,
    version: &str,
    arch: &str,
) -> Result<PathBuf> {
    let msi_out = bundle_root.join(format!("Ordia-{version}-windows-{arch}.msi"));
    let wixobj = bundle_root.join("windows").join("Ordia.wixobj");
    let candle = Command::new("candle")
        .arg(wxs_path)
        .arg("-out")
        .arg(&wixobj)
        .current_dir(root)
        .status()
        .context("failed to invoke WiX `candle.exe`")?;
    if !candle.success() {
        bail!("WiX `candle.exe` failed");
    }
    let light = Command::new("light")
        .arg(&wixobj)
        .arg("-out")
        .arg(&msi_out)
        .current_dir(root)
        .status()
        .context("failed to invoke WiX `light.exe`")?;
    if !light.success() {
        bail!("WiX `light.exe` failed");
    }
    Ok(msi_out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn third_party_license_report_and_wix_manifest_are_well_formed() {
        let entries = vec![ThirdPartyCrateEntry {
            name: "ironcalc".to_string(),
            version: "0.9.1".to_string(),
            license: "MIT OR Apache-2.0".to_string(),
            repository: "https://github.com/ironcalc/IronCalc".to_string(),
        }];
        let report = format_third_party_licenses_report(&entries);
        assert!(report.contains("ironcalc | 0.9.1 | MIT OR Apache-2.0"));

        let wxs = render_windows_wix_manifest(
            "0.1.0",
            Path::new("target/release/ordia-app.exe"),
            Path::new("assets/icons/Ordia.ico"),
            Path::new("assets/THIRD_PARTY_LICENSES.txt"),
        );
        assert!(wxs.contains("Ordia.Spreadsheet.xlsx"));
        assert!(wxs.contains("Ordia.Spreadsheet.csv"));
        assert!(wxs.contains("THIRD_PARTY_LICENSES.txt"));
    }
}
