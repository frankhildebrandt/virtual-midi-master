use anyhow::{Context, Result};
use virtual_midi_core::VirtualDevice;
use virtual_midi_engine::{
    ConfigStore, EngineController, acquire_single_instance, serve, set_autostart,
};
use virtual_midi_platform::create_system_backend;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "virtual_midi_engine=info".into()),
        )
        .init();
    // Acquire this before configuration loading or MIDI endpoint creation so a
    // racing second process cannot briefly publish duplicate virtual devices.
    let _instance = acquire_single_instance()?;
    let path = std::env::var_os("VMM_CONFIG_DIR")
        .map(std::path::PathBuf::from)
        .or_else(dirs::config_dir)
        .context("configuration directory is unavailable")?
        .join("virtual-midi-master/config.json");
    let store = ConfigStore::new(path);
    let loaded = store.load()?;
    if let Some(path) = &loaded.recovered_invalid_file {
        tracing::warn!(path = %path.display(), "recovered invalid configuration");
    }
    let mut config = loaded.config;
    if config.virtual_devices.is_empty() {
        config.virtual_devices.push(VirtualDevice {
            id: uuid::Uuid::new_v4(),
            name: "Virtual MIDI Master".into(),
            enabled: true,
            effects: Vec::new(),
        });
        store.save(&config)?;
    }
    let backend = create_system_backend(8192)?;
    let controller = EngineController::start(backend, store, config.clone())?;
    if config.autostart
        && std::env::var_os("VMM_DISABLE_AUTOSTART").is_none()
        && let Err(error) = set_autostart(true)
    {
        tracing::warn!(%error, "could not register autostart");
    }
    tracing::info!(endpoint = %virtual_midi_engine::ipc_endpoint().display(), "engine ready");
    serve(controller)
}
