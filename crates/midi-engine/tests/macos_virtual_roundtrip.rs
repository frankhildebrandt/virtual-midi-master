#![cfg(target_os = "macos")]

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use midir::{Ignore, MidiInput, MidiInputPort, MidiOutput, MidiOutputPort};
use tempfile::tempdir;
use uuid::Uuid;
use virtual_midi_core::{Config, EndpointId, Route, VirtualDevice};
use virtual_midi_engine::{ConfigStore, EngineController};
use virtual_midi_platform::create_system_backend;

#[test]
#[ignore = "requires a live macOS CoreMIDI service"]
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
    assert!(controller.snapshot().endpoints.iter().all(|endpoint| {
        endpoint.kind != virtual_midi_core::EndpointKind::Physical
            || (endpoint.name != format!("{device_name} MIDI In")
                && endpoint.name != format!("{device_name} MIDI Out"))
    }));

    let (mut receiving_client, receiving_port) =
        wait_for_input_port(&format!("{device_name} MIDI Out"));
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

    let (sending_client, sending_port) = wait_for_output_port(&format!("{device_name} MIDI In"));
    let mut sending_connection = sending_client
        .connect(&sending_port, "VMM roundtrip sender")
        .unwrap();
    let note = vec![0x90, 64, 100];
    sending_connection.send(&note).unwrap();

    assert_eq!(
        received_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        note
    );
    controller.shutdown();
}

fn wait_for_input_port(name: &str) -> (MidiInput, MidiInputPort) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let input = MidiInput::new("VMM roundtrip input discovery").unwrap();
        if let Some(port) = input
            .ports()
            .into_iter()
            .find(|port| input.port_name(port).as_deref() == Ok(name))
        {
            return (input, port);
        }
        assert!(
            Instant::now() < deadline,
            "MIDI input port '{name}' not found"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

fn wait_for_output_port(name: &str) -> (MidiOutput, MidiOutputPort) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let output = MidiOutput::new("VMM roundtrip output discovery").unwrap();
        if let Some(port) = output
            .ports()
            .into_iter()
            .find(|port| output.port_name(port).as_deref() == Ok(name))
        {
            return (output, port);
        }
        assert!(
            Instant::now() < deadline,
            "MIDI output port '{name}' not found"
        );
        thread::sleep(Duration::from_millis(20));
    }
}
