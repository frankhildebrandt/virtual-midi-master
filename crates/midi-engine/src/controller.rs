use std::collections::VecDeque;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossbeam_channel::{Receiver, Sender, bounded, select, tick};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use virtual_midi_core::{Config, EndpointRef, MidiEvent, Router};
use virtual_midi_platform::{EndpointInfo, MidiBackend, PlatformStatus};

use crate::{ConfigStore, set_autostart};

const MONITOR_CAPACITY: usize = 100;
const MONITOR_DATA_PREVIEW_BYTES: usize = 64;
const SNAPSHOT_PUBLISH_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EngineStats {
    pub received: u64,
    pub sent: u64,
    pub send_errors: u64,
    pub queue_overflows: u64,
    pub last_internal_latency_micros: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MonitorEvent {
    pub event: MidiEvent,
    pub data_len: usize,
    pub targets: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EngineSnapshot {
    pub revision: u64,
    pub running: bool,
    pub config: Config,
    pub endpoints: Vec<EndpointInfo>,
    pub platform: PlatformStatus,
    pub stats: EngineStats,
    pub monitor: Vec<MonitorEvent>,
    pub logs: Vec<String>,
    pub last_error: Option<String>,
}

enum Command {
    Apply(Config, Sender<Result<()>>),
    Shutdown,
}

#[derive(Clone)]
pub struct EngineController {
    commands: Sender<Command>,
    snapshot: Arc<RwLock<EngineSnapshot>>,
}

impl EngineController {
    pub fn start(
        mut backend: Box<dyn MidiBackend>,
        store: ConfigStore,
        mut initial_config: Config,
    ) -> Result<Self> {
        let router = Router::compile(&initial_config).context("compile initial routing")?;
        let endpoints = backend
            .reconfigure(&initial_config)
            .context("configure MIDI platform")?;
        remember_endpoints(&mut initial_config, &endpoints);
        store.save(&initial_config)?;
        let platform = backend.status();
        let snapshot = Arc::new(RwLock::new(EngineSnapshot {
            revision: 1,
            running: true,
            config: initial_config.clone(),
            endpoints: include_offline_endpoints(endpoints, &initial_config.known_endpoints),
            platform,
            stats: EngineStats::default(),
            monitor: Vec::new(),
            logs: vec!["Engine gestartet".into()],
            last_error: None,
        }));
        let (commands, receiver) = bounded(16);
        let worker_snapshot = snapshot.clone();
        thread::Builder::new()
            .name("virtual-midi-routing".into())
            .spawn(move || {
                run_worker(
                    &mut *backend,
                    store,
                    initial_config,
                    router,
                    receiver,
                    worker_snapshot,
                )
            })
            .context("start routing thread")?;
        Ok(Self { commands, snapshot })
    }

    pub fn snapshot(&self) -> EngineSnapshot {
        self.snapshot.read().clone()
    }

    pub fn apply_config(&self, config: Config) -> Result<()> {
        config.validate().context("validate configuration")?;
        let (tx, rx) = bounded(1);
        self.commands
            .send(Command::Apply(config, tx))
            .context("routing thread stopped")?;
        rx.recv_timeout(Duration::from_secs(10))
            .context("configuration apply timed out")??;
        Ok(())
    }

    pub fn shutdown(&self) {
        let _ = self.commands.try_send(Command::Shutdown);
    }
}

fn run_worker(
    backend: &mut dyn MidiBackend,
    store: ConfigStore,
    mut config: Config,
    mut router: Router,
    commands: Receiver<Command>,
    snapshot: Arc<RwLock<EngineSnapshot>>,
) {
    let heartbeat = tick(Duration::from_millis(1));
    let mut monitor = VecDeque::with_capacity(MONITOR_CAPACITY);
    let mut stats = EngineStats::default();
    let mut runtime_dirty = false;
    let mut send_errors_since_publish = 0_u64;
    let mut last_snapshot_publish = Instant::now();
    let mut last_refresh = Instant::now();
    loop {
        select! {
            recv(commands) -> command => match command {
                Ok(Command::Apply(next, response)) => {
                    if next.virtual_devices != config.virtual_devices {
                        for output in router.release_generated_sustain() {
                            if backend.send(&output.target, &output.data).is_ok() {
                                stats.sent += 1;
                            } else {
                                stats.send_errors += 1;
                                send_errors_since_publish += 1;
                            }
                        }
                        runtime_dirty = true;
                    }
                    let result = apply_transaction(backend, &store, &config, &next)
                        .map(|(new_router, committed, endpoints)| {
                            router = new_router;
                            config = committed.clone();
                            let mut current = snapshot.write();
                            current.config = committed.clone();
                            current.endpoints = include_offline_endpoints(endpoints, &committed.known_endpoints);
                            current.platform = backend.status();
                            current.last_error = None;
                            current.revision += 1;
                            append_log(&mut current, "Konfiguration angewendet".into());
                        });
                    if let Err(error) = &result {
                        let mut current = snapshot.write();
                        current.last_error = Some(error.to_string());
                        current.revision += 1;
                        append_log(&mut current, format!("Fehler: {error}"));
                    }
                    let _ = response.send(result);
                }
                Ok(Command::Shutdown) | Err(_) => break,
            },
            recv(heartbeat) -> _ => {
                let dropped = backend.take_queue_overflows();
                if dropped > 0 {
                    stats.queue_overflows += dropped;
                    runtime_dirty = true;
                }
                if last_refresh.elapsed() >= Duration::from_secs(2) {
                    match backend.refresh() {
                        Ok(Some(endpoints)) => {
                            remember_endpoints(&mut config, &endpoints);
                            if let Err(error) = store.save(&config) {
                                snapshot.write().last_error = Some(error.to_string());
                            }
                            let mut current = snapshot.write();
                            current.config = config.clone();
                            current.endpoints = include_offline_endpoints(endpoints, &config.known_endpoints);
                            current.revision += 1;
                            append_log(&mut current, "MIDI-Endpunkte aktualisiert".into());
                        }
                        Ok(None) => {}
                        Err(error) => {
                            let mut current = snapshot.write();
                            current.last_error = Some(error.to_string());
                            current.revision += 1;
                            append_log(&mut current, format!("Hotplug-Fehler: {error}"));
                        }
                    }
                    last_refresh = Instant::now();
                }
                let due = router.poll_due();
                if !due.is_empty() {
                    let mut send_errors = 0_u64;
                    for output in &due {
                        if backend.send(&output.target, &output.data).is_err() {
                            send_errors += 1;
                        }
                    }
                    stats.sent += (due.len() as u64).saturating_sub(send_errors);
                    stats.send_errors += send_errors;
                    send_errors_since_publish += send_errors;
                    runtime_dirty = true;
                }
                for _ in 0..1024 {
                    let Some(event) = backend.try_receive() else { break };
                    let started = Instant::now();
                    let monitor_event = MidiEvent::new(
                        event.source.clone(),
                        event.timestamp_micros,
                        event.data[..event.data.len().min(MONITOR_DATA_PREVIEW_BYTES)].to_vec(),
                    );
                    let data_len = event.data.len();
                    let routed = router.process(event);
                    let mut send_errors = 0_u64;
                    for output in &routed {
                        if backend.send(&output.target, &output.data).is_err() {
                            send_errors += 1;
                        }
                    }
                    if monitor.len() == MONITOR_CAPACITY {
                        monitor.pop_front();
                    }
                    monitor.push_back(MonitorEvent {
                        event: monitor_event,
                        data_len,
                        targets: routed.len(),
                    });
                    stats.received += 1;
                    stats.sent += (routed.len() as u64).saturating_sub(send_errors);
                    stats.send_errors += send_errors;
                    stats.last_internal_latency_micros = started.elapsed().as_micros() as u64;
                    send_errors_since_publish += send_errors;
                    runtime_dirty = true;
                }
                if runtime_dirty && last_snapshot_publish.elapsed() >= SNAPSHOT_PUBLISH_INTERVAL {
                    publish_runtime_snapshot(
                        &snapshot,
                        &stats,
                        &monitor,
                        send_errors_since_publish,
                    );
                    runtime_dirty = false;
                    send_errors_since_publish = 0;
                    last_snapshot_publish = Instant::now();
                }
            }
        }
    }
    let mut current = snapshot.write();
    current.stats = stats;
    current.monitor = monitor.into_iter().collect();
    current.running = false;
    current.revision += 1;
}

fn publish_runtime_snapshot(
    snapshot: &RwLock<EngineSnapshot>,
    stats: &EngineStats,
    monitor: &VecDeque<MonitorEvent>,
    send_errors: u64,
) {
    let mut current = snapshot.write();
    current.stats = stats.clone();
    current.monitor = monitor.iter().cloned().collect();
    current.revision += 1;
    if send_errors > 0 {
        append_log(
            &mut current,
            format!("{send_errors} MIDI-Nachricht(en) konnten nicht gesendet werden"),
        );
    }
}

fn append_log(snapshot: &mut EngineSnapshot, message: String) {
    const LOG_CAPACITY: usize = 100;
    if snapshot.logs.len() == LOG_CAPACITY {
        snapshot.logs.remove(0);
    }
    snapshot.logs.push(message);
}

fn apply_transaction(
    backend: &mut dyn MidiBackend,
    store: &ConfigStore,
    previous: &Config,
    next: &Config,
) -> Result<(Router, Config, Vec<EndpointInfo>)> {
    let router = Router::compile(next).context("compile routing")?;
    let endpoints = match backend.reconfigure(next) {
        Ok(endpoints) => endpoints,
        Err(error) => {
            backend.reconfigure(previous).context(
                "configuration failed and restoring previous MIDI endpoints also failed",
            )?;
            return Err(error).context("prepare MIDI endpoints");
        }
    };
    if next.autostart != previous.autostart {
        if let Err(error) = set_autostart(next.autostart) {
            backend
                .reconfigure(previous)
                .context("autostart failed and restoring previous MIDI endpoints also failed")?;
            return Err(error).context("update autostart registration");
        }
    }
    let mut committed = next.clone();
    remember_endpoints(&mut committed, &endpoints);
    if let Err(error) = store.save(&committed) {
        let backend_rollback = backend.reconfigure(previous).map(|_| ());
        let autostart_rollback = if next.autostart != previous.autostart {
            set_autostart(previous.autostart)
        } else {
            Ok(())
        };
        if let (Err(backend_error), Err(autostart_error)) = (&backend_rollback, &autostart_rollback)
        {
            return Err(anyhow::anyhow!(
                "persist configuration: {error}; restoring MIDI endpoints failed: {backend_error}; restoring autostart failed: {autostart_error}"
            ));
        }
        backend_rollback
            .context("persistence failed and restoring previous MIDI endpoints also failed")?;
        autostart_rollback
            .context("persistence failed and restoring previous autostart also failed")?;
        return Err(error).context("persist configuration");
    }
    Ok((router, committed, endpoints))
}

fn remember_endpoints(config: &mut Config, endpoints: &[EndpointInfo]) {
    for endpoint in endpoints {
        let reference = EndpointRef {
            id: endpoint.id.clone(),
            direction: endpoint.direction,
            kind: endpoint.kind,
            last_known_name: endpoint.name.clone(),
        };
        if let Some(existing) = config
            .known_endpoints
            .iter_mut()
            .find(|known| known.id == endpoint.id)
        {
            *existing = reference;
        } else {
            config.known_endpoints.push(reference);
        }
    }
}

fn include_offline_endpoints(
    mut live: Vec<EndpointInfo>,
    known: &[EndpointRef],
) -> Vec<EndpointInfo> {
    for endpoint in known {
        if live.iter().any(|current| current.id == endpoint.id) {
            continue;
        }
        live.push(EndpointInfo {
            id: endpoint.id.clone(),
            name: endpoint.last_known_name.clone(),
            direction: endpoint.direction,
            kind: endpoint.kind,
            online: false,
        });
    }
    live
}
