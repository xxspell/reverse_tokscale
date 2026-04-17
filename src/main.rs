use std::path::Path;

use tokscale_activity_emulator::orchestrator::run_once_with_config_path;

fn main() -> anyhow::Result<()> {
    let config_path = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("TOKSCALE_CONFIG").ok())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "config path is required: pass as first argument or set TOKSCALE_CONFIG"
            )
        })?;

    run_once_with_config_path(Path::new(&config_path))?;
    Ok(())
}
