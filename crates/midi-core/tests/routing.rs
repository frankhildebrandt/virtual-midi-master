use uuid::Uuid;
use virtual_midi_core::{
    Config, ConfigError, DissonanceSensitivity, EndpointId, MessageType, MidiEffect, MidiEvent,
    Route, Router, ScaleType, SustainMode, TransformRule, VirtualDevice,
};

fn route(rules: Vec<TransformRule>) -> (Config, EndpointId, EndpointId) {
    let source = EndpointId::new("physical:input:keyboard");
    let target = EndpointId::new("virtual:main:out");
    let config = Config {
        routes: vec![Route {
            id: Uuid::new_v4(),
            source: source.clone(),
            target: target.clone(),
            enabled: true,
            rules,
        }],
        ..Config::default()
    };
    (config, source, target)
}

#[test]
fn rejects_enabled_cycles_but_ignores_disabled_routes() {
    let a = EndpointId::new("a");
    let b = EndpointId::new("b");
    let mut config = Config {
        routes: vec![
            Route {
                id: Uuid::new_v4(),
                source: a.clone(),
                target: b.clone(),
                enabled: true,
                rules: vec![],
            },
            Route {
                id: Uuid::new_v4(),
                source: b,
                target: a,
                enabled: true,
                rules: vec![],
            },
        ],
        ..Config::default()
    };

    assert!(matches!(
        Router::compile(&config),
        Err(ConfigError::RoutingCycle(_))
    ));
    config.routes[1].enabled = false;
    assert!(Router::compile(&config).is_ok());
}

#[test]
fn distinguishes_clock_from_other_realtime_messages_and_filters_sysex() {
    let (config, source, _) = route(vec![TransformRule::DropMessageType {
        message_type: MessageType::MidiClock,
    }]);
    let router = Router::compile(&config).unwrap();
    assert!(
        router
            .process(MidiEvent::new(source.clone(), 1, vec![0xF8]))
            .is_empty()
    );
    assert_eq!(
        router
            .process(MidiEvent::new(source.clone(), 1, vec![0xFA]))
            .len(),
        1
    );

    let (config, source, _) = route(vec![TransformRule::DropMessageType {
        message_type: MessageType::SysEx,
    }]);
    let router = Router::compile(&config).unwrap();
    assert!(
        router
            .process(MidiEvent::new(source, 1, vec![0xF0, 0x7D, 0x01, 0xF7]))
            .is_empty()
    );
}

#[test]
fn routes_an_unmodified_message_to_the_configured_target() {
    let (config, source, target) = route(vec![]);
    let router = Router::compile(&config).unwrap();

    let result = router.process(MidiEvent::new(source, 42, vec![0x90, 60, 100]));

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].target, target);
    assert_eq!(result[0].timestamp_micros, 42);
    assert_eq!(result[0].data, vec![0x90, 60, 100]);
}

#[test]
fn applies_rules_in_order_and_can_drop_pitch_bend() {
    let (config, source, _) = route(vec![
        TransformRule::MapChannel {
            from: Some(0),
            to: 4,
        },
        TransformRule::DropMessageType {
            message_type: MessageType::PitchBend,
        },
    ]);
    let router = Router::compile(&config).unwrap();

    let notes = router.process(MidiEvent::new(source.clone(), 1, vec![0x90, 64, 127]));
    let pitch = router.process(MidiEvent::new(source, 2, vec![0xE0, 0, 64]));

    assert_eq!(notes[0].data, vec![0x94, 64, 127]);
    assert!(pitch.is_empty());
}

#[test]
fn fans_out_without_sharing_mutated_message_buffers() {
    let (mut config, source, first_target) = route(vec![TransformRule::MapControlChange {
        from: 1,
        to: 74,
        channel: None,
    }]);
    let second_target = EndpointId::new("physical:output:synth");
    config.routes.push(Route {
        id: Uuid::new_v4(),
        source: source.clone(),
        target: second_target.clone(),
        enabled: true,
        rules: vec![],
    });
    let router = Router::compile(&config).unwrap();

    let result = router.process(MidiEvent::new(source, 1, vec![0xB0, 1, 99]));

    assert_eq!(result[0].target, first_target);
    assert_eq!(result[0].data, vec![0xB0, 74, 99]);
    assert_eq!(result[1].target, second_target);
    assert_eq!(result[1].data, vec![0xB0, 1, 99]);
}

fn device_route(effects: Vec<MidiEffect>) -> (Router, EndpointId, EndpointId) {
    let id = Uuid::new_v4();
    let source = EndpointId::new(format!("virtual:{id}:in"));
    let target = EndpointId::new("physical:output:synth");
    let config = Config {
        virtual_devices: vec![VirtualDevice {
            id,
            name: "Performance".into(),
            enabled: true,
            effects,
        }],
        routes: vec![Route {
            id: Uuid::new_v4(),
            source: source.clone(),
            target: target.clone(),
            enabled: true,
            rules: vec![],
        }],
        ..Config::default()
    };
    (Router::compile(&config).unwrap(), source, target)
}

#[test]
fn scale_helper_maps_note_on_and_matching_note_off() {
    let (router, source, _) = device_route(vec![MidiEffect::ScaleHelper {
        enabled: true,
        root: 2,
        scale: ScaleType::NaturalMinor,
    }]);

    let note_on = router.process(MidiEvent::new(source.clone(), 1, vec![0x90, 60, 100]));
    let note_off = router.process(MidiEvent::new(source, 2, vec![0x80, 60, 0]));

    assert_eq!(note_on[0].data, vec![0x90, 62, 100]);
    assert_eq!(note_off[0].data, vec![0x80, 62, 0]);
}

#[test]
fn sustain_helper_releases_and_represses_for_a_new_chord() {
    let (router, source, _) = device_route(vec![MidiEffect::SustainHelper {
        enabled: true,
        mode: SustainMode::ChordGesture,
        min_sustain_notes: 2,
        release_time_ms: 5,
        dissonance: DissonanceSensitivity::Off,
        auto_sustain: true,
    }]);

    let first = router.process(MidiEvent::new(source.clone(), 1, vec![0x90, 60, 100]));
    assert_eq!(
        first.iter().map(|event| &event.data).collect::<Vec<_>>(),
        vec![&vec![0xB0, 64, 127], &vec![0x90, 60, 100]]
    );
    router.process(MidiEvent::new(source.clone(), 2, vec![0x90, 64, 100]));
    router.process(MidiEvent::new(source.clone(), 3, vec![0x80, 60, 0]));
    router.process(MidiEvent::new(source.clone(), 4, vec![0x80, 64, 0]));

    let next = router.process(MidiEvent::new(source, 5, vec![0x90, 67, 100]));
    assert_eq!(
        next.iter().map(|event| &event.data).collect::<Vec<_>>(),
        vec![&vec![0xB0, 64, 0], &vec![0x90, 67, 100]]
    );
    std::thread::sleep(std::time::Duration::from_millis(7));
    assert_eq!(router.poll_due()[0].data, vec![0xB0, 64, 127]);
    assert_eq!(
        router.release_generated_sustain()[0].data,
        vec![0xB0, 64, 0]
    );
    assert!(router.release_generated_sustain().is_empty());
}
