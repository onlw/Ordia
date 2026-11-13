//! Main executable entry point for the Ordia desktop client.

fn main() -> anyhow::Result<()> {
    println!("{} {}", ordia_app::app_name(), env!("CARGO_PKG_VERSION"));
    Ok(())
}
