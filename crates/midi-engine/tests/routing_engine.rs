use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tempfile::tempdir;
use uuid::Uuid;
use virtual_midi_core::{Config, EndpointId, MidiEvent, Route, VirtualDevice};
use virtual_midi_engine::{ConfigStore, EngineController};
use virtual_midi_platform::{
    EndpointInfo, MemoryBackend, MidiBackend, PlatformError, PlatformStatus,
};

#[test]
fn routes_backend_input_while_configuration_client_is_disconnected() {
    let source = EndpointId::new("source");
    let target = EndpointId::new("target");
    let config = Config {
        routes: vec![Route {
            id: Uuid::new_v4(),
            source: source.clone(),
            target: target.clone(),
            enabled: true,
            rules: vec![],
        }],
        ..Config::default()
    };
    let backend = MemoryBackend::new(8);
    let observer = backend.clone();
    let directory = tempdir().unwrap();
    let controller = EngineController::start(
        Box::new(backend),
        ConfigStore::new(directory.path().join("config.json")),
        config,
    )
    .unwrap();

    assert!(observer.inject(MidiEvent::new(source, 12, vec![0x90, 60, 100])));
    let deadline = Instant::now() + Duration::from_secs(1);
    while (observer.sent_to(&target).is_empty() || controller.snapshot().stats.sent == 0)
        && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(2));
    }

    assert_eq!(observer.sent_to(&target), vec![vec![0x90, 60, 100]]);
    assert_eq!(controller.snapshot().stats.sent, 1);
    controller.shutdown();
}

#[test]
fn throttles_observable_snapshots_without_losing_a_midi_burst() {
    let source = EndpointId::new("burst-source");
    let target = EndpointId::new("burst-target");
    let config = Config {
        routes: vec![Route {
            id: Uuid::new_v4(),
            source: source.clone(),
            target: target.clone(),
            enabled: true,
            rules: vec![],
        }],
        ..Config::default()
    };
    let backend = MemoryBackend::new(512);
    let observer = backend.clone();
    let directory = tempdir().unwrap();
    let controller = EngineController::start(
        Box::new(backend),
        ConfigStore::new(directory.path().join("config.json")),
        config,
    )
    .unwrap();

    for timestamp in 0..200 {
        assert!(observer.inject(MidiEvent::new(
            source.clone(),
            timestamp,
            vec![0x90, 60, 100],
        )));
    }
    let deadline = Instant::now() + Duration::from_secs(1);
    while controller.snapshot().stats.received < 200 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(2));
    }

    let snapshot = controller.snapshot();
    assert_eq!(observer.sent_to(&target).len(), 200);
    assert_eq!(snapshot.stats.received, 200);
    assert!(
        snapshot.revision <= 5,
        "a 200-event burst produced {} observable revisions",
        snapshot.revision
    );
    controller.shutdown();
}

#[test]
fn routes_large_sysex_intact_but_bounds_the_ui_monitor_preview() {
    let source = EndpointId::new("sysex-source");
    let target = EndpointId::new("sysex-target");
    let config = Config {
        routes: vec![Route {
            id: Uuid::new_v4(),
            source: source.clone(),
            target: target.clone(),
            enabled: true,
            rules: vec![],
        }],
        ..Config::default()
    };
    let backend = MemoryBackend::new(8);
    let observer = backend.clone();
    let directory = tempdir().unwrap();
    let controller = EngineController::start(
        Box::new(backend),
        ConfigStore::new(directory.path().join("config.json")),
        config,
    )
    .unwrap();
    let mut sysex = vec![0xF0];
    sysex.extend(std::iter::repeat_n(0x7D, 256 * 1024));
    sysex.push(0xF7);
    assert!(observer.inject(MidiEvent::new(source, 1, sysex.clone())));

    let deadline = Instant::now() + Duration::from_secs(1);
    while controller.snapshot().stats.sent == 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(2));
    }
    let snapshot = controller.snapshot();

    assert_eq!(observer.sent_to(&target), vec![sysex.clone()]);
    assert_eq!(snapshot.monitor[0].data_len, sysex.len());
    assert_eq!(snapshot.monitor[0].event.data.len(), 64);
    controller.shutdown();
}

#[test]
fn reports_bounded_input_queue_overflow() {
    let backend = MemoryBackend::new(1);
    assert!(backend.inject(MidiEvent::new(EndpointId::new("source"), 1, vec![0xF8])));
    assert!(!backend.inject(MidiEvent::new(EndpointId::new("source"), 2, vec![0xF8])));
    assert_eq!(backend.take_queue_overflows(), 1);
    assert_eq!(backend.take_queue_overflows(), 0);
}

struct FailingBackend {
    active_name: Arc<Mutex<String>>,
}

impl MidiBackend for FailingBackend {
    fn reconfigure(&mut self, config: &Config) -> Result<Vec<EndpointInfo>, PlatformError> {
        let name = config
            .virtual_devices
            .first()
            .map(|device| device.name.clone())
            .unwrap_or_else(|| "default".into());
        *self.active_name.lock().unwrap() = name.clone();
        if name == "fail" {
            return Err(PlatformError::Endpoint("injected failure".into()));
        }
        Ok(Vec::new())
    }

    fn endpoints(&self) -> Vec<EndpointInfo> {
        Vec::new()
    }
    fn try_receive(&self) -> Option<MidiEvent> {
        None
    }
    fn send(&mut self, _target: &EndpointId, _data: &[u8]) -> Result<(), PlatformError> {
        Ok(())
    }
    fn status(&self) -> PlatformStatus {
        PlatformStatus {
            platform: "failure-test".into(),
            runtime_available: true,
            detail: String::new(),
        }
    }
}

#[test]
fn failed_platform_reconfiguration_restores_previous_endpoints() {
    let active_name = Arc::new(Mutex::new(String::new()));
    let backend = FailingBackend {
        active_name: active_name.clone(),
    };
    let directory = tempdir().unwrap();
    let controller = EngineController::start(
        Box::new(backend),
        ConfigStore::new(directory.path().join("config.json")),
        Config::default(),
    )
    .unwrap();
    let mut failing = Config::default();
    failing.virtual_devices.push(VirtualDevice {
        id: Uuid::new_v4(),
        name: "fail".into(),
        enabled: true,
    });

    assert!(controller.apply_config(failing).is_err());
    assert_eq!(&*active_name.lock().unwrap(), "default");
    assert_eq!(controller.snapshot().config, Config::default());
    controller.shutdown();
}
