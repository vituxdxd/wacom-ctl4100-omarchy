mod actions;
mod driver;
mod hyprland;
mod server;

use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;

use wacom_proto::ipc::Status;

fn main() -> anyhow::Result<()> {
    hyprland::ensure_instance_env();
    let config_path = std::env::var_os("WACOMD_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(wacom_proto::config::default_path);
    let config = wacom_proto::config::load(&config_path)?;
    let status = Status::disconnected(config_path.display().to_string());
    let shared = server::Shared::new(config.clone(), status, config_path.clone());

    let (cmd_tx, cmd_rx) = mpsc::channel();
    server::spawn(Arc::clone(&shared), cmd_tx)?;

    let (hypr_tx, hypr_rx) = mpsc::channel();
    hyprland::spawn_listener(hypr_tx);

    let mut driver = driver::Driver::new(shared, cmd_rx, hypr_rx, config, config_path)?;
    driver.run();
    Ok(())
}
