use std::time::{Duration, Instant};

use uuid::Uuid;
use virtual_midi_core::{Config, EndpointId, MidiEvent, Route, Router};

#[test]
fn routes_ten_thousand_events_without_loss_and_below_one_millisecond_p95() {
    let source = EndpointId::new("performance-source");
    let config = Config {
        routes: vec![Route {
            id: Uuid::new_v4(),
            source: source.clone(),
            target: EndpointId::new("performance-target"),
            enabled: true,
            rules: vec![],
        }],
        ..Config::default()
    };
    let router = Router::compile(&config).unwrap();
    let mut timings = Vec::with_capacity(10_000);
    let mut delivered = 0;
    for timestamp in 0..10_000 {
        let started = Instant::now();
        delivered += router
            .process(MidiEvent::new(
                source.clone(),
                timestamp,
                vec![0x90, 60, 100],
            ))
            .len();
        timings.push(started.elapsed().as_micros());
    }
    timings.sort_unstable();

    assert_eq!(delivered, 10_000);
    assert!(timings[9_499] < 1_000, "p95 was {} µs", timings[9_499]);
}

#[test]
fn preserves_a_large_sysex_message_byte_for_byte() {
    let source = EndpointId::new("sysex-source");
    let config = Config {
        routes: vec![Route {
            id: Uuid::new_v4(),
            source: source.clone(),
            target: EndpointId::new("sysex-target"),
            enabled: true,
            rules: vec![],
        }],
        ..Config::default()
    };
    let router = Router::compile(&config).unwrap();
    let mut sysex = vec![0xF0];
    sysex.extend(std::iter::repeat_n(0x7D, 256 * 1024));
    sysex.push(0xF7);

    let output = router.process(MidiEvent::new(source, 0, sysex.clone()));
    assert_eq!(output[0].data, sysex);
}

#[test]
#[ignore = "ten-minute hardware-independent acceptance soak"]
fn routes_ten_thousand_events_per_second_for_ten_minutes() {
    const SECONDS: usize = 10 * 60;
    const EVENTS_PER_SECOND: usize = 10_000;
    let source = EndpointId::new("soak-source");
    let config = Config {
        routes: vec![Route {
            id: Uuid::new_v4(),
            source: source.clone(),
            target: EndpointId::new("soak-target"),
            enabled: true,
            rules: vec![],
        }],
        ..Config::default()
    };
    let router = Router::compile(&config).unwrap();
    let mut timings = Vec::with_capacity(SECONDS * EVENTS_PER_SECOND);
    let mut delivered = 0_usize;

    for second in 0..SECONDS {
        let interval_started = Instant::now();
        for event in 0..EVENTS_PER_SECOND {
            let started = Instant::now();
            delivered += router
                .process(MidiEvent::new(
                    source.clone(),
                    (second * EVENTS_PER_SECOND + event) as u64,
                    vec![0x90, 60, 100],
                ))
                .len();
            timings.push(started.elapsed().as_micros() as u64);
        }
        if let Some(remaining) = Duration::from_secs(1).checked_sub(interval_started.elapsed()) {
            std::thread::sleep(remaining);
        }
    }

    timings.sort_unstable();
    let p95 = timings[timings.len() * 95 / 100];
    assert_eq!(delivered, SECONDS * EVENTS_PER_SECOND);
    assert!(p95 < 1_000, "p95 was {p95} µs");
}
