#![cfg(target_os = "windows")]

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use midir::{Ignore, MidiInput, MidiInputPort, MidiOutput, MidiOutputPort};
use tempfile::tempdir;
use uuid::Uuid;
use virtual_midi_core::{Config, EndpointId, EndpointKind, Route, VirtualDevice};
use virtual_midi_engine::{ConfigStore, EngineController};
use virtual_midi_platform::create_system_backend;

#[test]
#[ignore = "requires Windows 11 24H2/25H2 with Windows MIDI Services"]
fn routes_a_note_between_system_visible_virtual_ports() {
    let device_id = Uuid::new_v4();
    let device_name = format!("VMM Roundtrip {}", &device_id.to_string()[..8]);
    let source = EndpointId::new(format!("virtual:{device_id}:in"));
    let target = EndpointId::new(format!("virtual:{device_id}:out"));
    let config = Config {
        virtual_devices: vec![VirtualDevice {
            id: device_id,
            name: device_name.clone(),
            enabled: true,
            effects: Vec::new(),
        }],
        routes: vec![Route {
            id: Uuid::new_v4(),
            source,
            target,
            enabled: true,
            rules: vec![],
        }],
        autostart: false,
        ..Config::default()
    };
    let directory = tempdir().unwrap();
    let controller = EngineController::start(
        create_system_backend(256).unwrap(),
        ConfigStore::new(directory.path().join("config.json")),
        config,
    )
    .unwrap();
    let snapshot = controller.snapshot();
    assert!(
        snapshot.platform.runtime_available,
        "Windows MIDI Services runtime is unavailable: {} ({})",
        snapshot.platform.detail,
        snapshot.last_error.unwrap_or_default()
    );
    assert!(
        snapshot.last_error.is_none(),
        "engine started with MIDI error: {:?}",
        snapshot.last_error
    );
    assert!(snapshot.endpoints.iter().any(|endpoint| {
        endpoint.kind == EndpointKind::Virtual
            && endpoint.online
            && endpoint.name.ends_with("MIDI In")
    }));
    assert!(snapshot.endpoints.iter().all(|endpoint| {
        endpoint.kind != EndpointKind::Physical
            || (endpoint.name != format!("{device_name} MIDI In")
                && endpoint.name != format!("{device_name} MIDI Out")
                && endpoint.name != device_name)
    }));

    let (mut receiving_client, receiving_port) = wait_for_input_port(&device_name);
    receiving_client.ignore(Ignore::None);
    let (received_tx, received_rx) = mpsc::sync_channel(1);
    let _receiving_connection = receiving_client
        .connect(
            &receiving_port,
            "VMM roundtrip receiver",
            move |_timestamp, bytes, ()| {
                let _ = received_tx.try_send(bytes.to_vec());
            },
            (),
        )
        .unwrap();

    let (sending_client, sending_port) = wait_for_output_port(&device_name);
    let mut sending_connection = sending_client
        .connect(&sending_port, "VMM roundtrip sender")
        .unwrap();
    let note = vec![0x90, 64, 100];
    sending_connection.send(&note).unwrap();

    assert_eq!(
        received_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        note
    );
    controller.shutdown();
}

fn name_matches_virtual(port_name: &str, device_name: &str, output_side: bool) -> bool {
    let suffix = if output_side { "MIDI Out" } else { "MIDI In" };
    port_name == device_name
        || port_name == format!("{device_name} {suffix}")
        || port_name.starts_with(&format!("{device_name} ("))
        || port_name.starts_with(&format!("{device_name} ["))
}

fn wait_for_input_port(device_name: &str) -> (MidiInput, MidiInputPort) {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let input = MidiInput::new("VMM roundtrip input discovery").unwrap();
        if let Some(port) = input.ports().into_iter().find(|port| {
            input
                .port_name(port)
                .is_ok_and(|name| name_matches_virtual(&name, device_name, true))
        }) {
            return (input, port);
        }
        assert!(
            Instant::now() < deadline,
            "MIDI input port for '{device_name}' not found"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

fn wait_for_output_port(device_name: &str) -> (MidiOutput, MidiOutputPort) {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let output = MidiOutput::new("VMM roundtrip output discovery").unwrap();
        if let Some(port) = output.ports().into_iter().find(|port| {
            output
                .port_name(port)
                .is_ok_and(|name| name_matches_virtual(&name, device_name, false))
        }) {
            return (output, port);
        }
        assert!(
            Instant::now() < deadline,
            "MIDI output port for '{device_name}' not found"
        );
        thread::sleep(Duration::from_millis(50));
    }
}
