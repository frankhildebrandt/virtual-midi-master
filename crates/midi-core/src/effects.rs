use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::{
    Config, DissonanceSensitivity, EndpointId, MidiEffect, MidiEvent, ScaleType, SustainMode,
};

#[derive(Clone, Debug, Default)]
pub(crate) struct EffectRack {
    chains: HashMap<EndpointId, Vec<EffectInstance>>,
}

impl EffectRack {
    pub(crate) fn compile(config: &Config) -> Self {
        let chains = config
            .virtual_devices
            .iter()
            .filter(|device| device.enabled && !device.effects.is_empty())
            .map(|device| {
                (
                    EndpointId::new(format!("virtual:{}:in", device.id)),
                    device
                        .effects
                        .iter()
                        .cloned()
                        .map(EffectInstance::new)
                        .collect(),
                )
            })
            .collect();
        Self { chains }
    }

    pub(crate) fn process(&mut self, event: MidiEvent) -> Vec<MidiEvent> {
        let Some(chain) = self.chains.get_mut(&event.source) else {
            return vec![event];
        };
        let mut events = vec![event];
        for effect in chain {
            events = events
                .into_iter()
                .flat_map(|event| effect.process(event))
                .collect();
        }
        events
    }

    pub(crate) fn poll_due(&mut self) -> Vec<MidiEvent> {
        let mut due = Vec::new();
        for (source, chain) in &mut self.chains {
            for index in 0..chain.len() {
                let generated = chain[index].poll_due(source);
                for event in generated {
                    let mut events = vec![event];
                    for later in &mut chain[index + 1..] {
                        events = events
                            .into_iter()
                            .flat_map(|event| later.process(event))
                            .collect();
                    }
                    due.extend(events);
                }
            }
        }
        due
    }

    pub(crate) fn release_generated_sustain(&mut self) -> Vec<MidiEvent> {
        let mut releases = Vec::new();
        for (source, chain) in &mut self.chains {
            for index in 0..chain.len() {
                let generated = chain[index].release_generated_sustain(source);
                for event in generated {
                    let mut events = vec![event];
                    for later in &mut chain[index + 1..] {
                        events = events
                            .into_iter()
                            .flat_map(|event| later.process(event))
                            .collect();
                    }
                    releases.extend(events);
                }
            }
        }
        releases
    }
}

#[derive(Clone, Debug)]
enum EffectInstance {
    Scale(ScaleHelper),
    Sustain(Box<SustainHelper>),
}

impl EffectInstance {
    fn new(config: MidiEffect) -> Self {
        match config {
            MidiEffect::ScaleHelper {
                enabled,
                root,
                scale,
            } => Self::Scale(ScaleHelper::new(enabled, root, scale)),
            MidiEffect::SustainHelper {
                enabled,
                mode,
                min_sustain_notes,
                release_time_ms,
                dissonance,
                auto_sustain,
            } => Self::Sustain(Box::new(SustainHelper::new(
                enabled,
                mode,
                min_sustain_notes,
                release_time_ms,
                dissonance,
                auto_sustain,
            ))),
        }
    }

    fn process(&mut self, event: MidiEvent) -> Vec<MidiEvent> {
        match self {
            Self::Scale(effect) => vec![effect.process(event)],
            Self::Sustain(effect) => effect.process(event),
        }
    }

    fn poll_due(&mut self, source: &EndpointId) -> Vec<MidiEvent> {
        match self {
            Self::Scale(_) => Vec::new(),
            Self::Sustain(effect) => effect.poll_due(source),
        }
    }

    fn release_generated_sustain(&mut self, source: &EndpointId) -> Vec<MidiEvent> {
        match self {
            Self::Scale(_) => Vec::new(),
            Self::Sustain(effect) => effect.release_generated_sustain(source),
        }
    }
}

#[derive(Clone, Debug)]
struct ScaleHelper {
    enabled: bool,
    root: u8,
    scale: ScaleType,
    note_map: Vec<Option<u8>>,
}

impl ScaleHelper {
    fn new(enabled: bool, root: u8, scale: ScaleType) -> Self {
        Self {
            enabled,
            root,
            scale,
            note_map: vec![None; 16 * 128],
        }
    }

    fn process(&mut self, mut event: MidiEvent) -> MidiEvent {
        if event.data.len() < 3 {
            return event;
        }
        let status = event.data[0] & 0xF0;
        if status != 0x80 && status != 0x90 {
            return event;
        }
        let channel = usize::from(event.data[0] & 0x0F);
        let note = event.data[1];
        let note_on = status == 0x90 && event.data[2] > 0;
        let index = channel * 128 + usize::from(note);
        if note_on {
            if self.enabled {
                let mapped = map_note_to_scale(note, self.root, self.scale);
                self.note_map[index] = Some(mapped);
                event.data[1] = mapped;
            }
        } else {
            let mapped = self.note_map[index].take().unwrap_or_else(|| {
                if self.enabled {
                    map_note_to_scale(note, self.root, self.scale)
                } else {
                    note
                }
            });
            event.data[1] = mapped;
        }
        event
    }
}

fn scale_intervals(scale: ScaleType) -> [i16; 7] {
    match scale {
        ScaleType::Major => [0, 2, 4, 5, 7, 9, 11],
        ScaleType::NaturalMinor => [0, 2, 3, 5, 7, 8, 10],
        ScaleType::HarmonicMinor => [0, 2, 3, 5, 7, 8, 11],
        ScaleType::MelodicMinor => [0, 2, 3, 5, 7, 9, 11],
    }
}

fn map_note_to_scale(note: u8, root: u8, scale: ScaleType) -> u8 {
    const WHITE_PCS: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
    let intervals = scale_intervals(scale);
    let pc = note % 12;
    if let Some(degree) = WHITE_PCS.iter().position(|candidate| *candidate == pc) {
        return (i16::from(note / 12) * 12 + i16::from(root) + intervals[degree]).clamp(0, 127)
            as u8;
    }
    let mut best = i16::from(note);
    let mut best_distance = 128_i16;
    let first_octave = (i16::from(note) - i16::from(root)).div_euclid(12) - 1;
    for octave in first_octave..first_octave + 4 {
        for interval in intervals {
            let candidate = octave * 12 + i16::from(root) + interval;
            if (0..=127).contains(&candidate) {
                let distance = (candidate - i16::from(note)).abs();
                if distance < best_distance || (distance == best_distance && candidate > best) {
                    best = candidate;
                    best_distance = distance;
                }
            }
        }
    }
    best as u8
}

#[derive(Clone, Debug)]
struct SustainChannel {
    held_notes: [u8; 128],
    held_pitch_classes: [u8; 12],
    active_note_count: u16,
    sustain_mask: u16,
    output_pedal_down: bool,
    generated_pedal_down: bool,
    repress_at: Option<Instant>,
    last_timestamp_micros: u64,
}

impl Default for SustainChannel {
    fn default() -> Self {
        Self {
            held_notes: [0; 128],
            held_pitch_classes: [0; 12],
            active_note_count: 0,
            sustain_mask: 0,
            output_pedal_down: false,
            generated_pedal_down: false,
            repress_at: None,
            last_timestamp_micros: 0,
        }
    }
}

#[derive(Clone, Debug)]
struct SustainHelper {
    enabled: bool,
    mode: SustainMode,
    min_sustain_notes: u8,
    release_time: Duration,
    dissonance: DissonanceSensitivity,
    auto_sustain: bool,
    channels: [SustainChannel; 16],
}

impl SustainHelper {
    fn new(
        enabled: bool,
        mode: SustainMode,
        min_notes: u8,
        release_ms: u16,
        dissonance: DissonanceSensitivity,
        auto_sustain: bool,
    ) -> Self {
        Self {
            enabled,
            mode,
            min_sustain_notes: min_notes,
            release_time: Duration::from_millis(u64::from(release_ms)),
            dissonance,
            auto_sustain,
            channels: std::array::from_fn(|_| SustainChannel::default()),
        }
    }

    fn process(&mut self, event: MidiEvent) -> Vec<MidiEvent> {
        if !self.enabled || event.data.len() < 3 {
            return vec![event];
        }
        let status = event.data[0] & 0xF0;
        let channel_index = usize::from(event.data[0] & 0x0F);
        let channel = &mut self.channels[channel_index];
        channel.last_timestamp_micros = event.timestamp_micros;
        let mut output = Vec::with_capacity(3);
        if status == 0x90 && event.data[2] > 0 {
            let note = event.data[1];
            let is_new_gesture = channel.active_note_count == 0;
            let enough_notes =
                channel.sustain_mask.count_ones() >= u32::from(self.min_sustain_notes);
            let gesture_reset = matches!(self.mode, SustainMode::ChordGesture | SustainMode::Both)
                && is_new_gesture;
            let dissonance_reset = matches!(self.mode, SustainMode::Dissonance | SustainMode::Both)
                && has_dissonance(note % 12, channel.sustain_mask, self.dissonance);
            if channel.output_pedal_down && enough_notes && (gesture_reset || dissonance_reset) {
                output.push(cc64(&event, 0));
                channel.output_pedal_down = false;
                channel.generated_pedal_down = false;
                channel.sustain_mask = 0;
                channel.repress_at = Some(Instant::now() + self.release_time);
            }
            track_note_on(channel, note);
            if self.auto_sustain && channel.repress_at.is_none() && !channel.output_pedal_down {
                output.push(cc64(&event, 127));
                channel.output_pedal_down = true;
                channel.generated_pedal_down = true;
                add_held_to_pool(channel);
            }
            if channel.output_pedal_down && channel.repress_at.is_none() {
                channel.sustain_mask |= 1 << (note % 12);
            }
        } else if status == 0x80 || status == 0x90 {
            track_note_off(channel, event.data[1]);
        } else if status == 0xB0 && event.data[1] == 64 {
            channel.output_pedal_down = event.data[2] >= 64;
            channel.generated_pedal_down = false;
            channel.repress_at = None;
            if channel.output_pedal_down {
                add_held_to_pool(channel);
            } else {
                channel.sustain_mask = 0;
            }
        } else if status == 0xB0 && matches!(event.data[1], 120 | 123) {
            *channel = SustainChannel::default();
        }
        output.push(event);
        output
    }

    fn poll_due(&mut self, source: &EndpointId) -> Vec<MidiEvent> {
        let now = Instant::now();
        self.channels
            .iter_mut()
            .enumerate()
            .filter_map(|(index, channel)| {
                if channel.repress_at.is_some_and(|deadline| deadline <= now) {
                    channel.repress_at = None;
                    channel.output_pedal_down = true;
                    channel.generated_pedal_down = true;
                    add_held_to_pool(channel);
                    Some(MidiEvent::new(
                        source.clone(),
                        channel.last_timestamp_micros,
                        vec![0xB0 | index as u8, 64, 127],
                    ))
                } else {
                    None
                }
            })
            .collect()
    }

    fn release_generated_sustain(&mut self, source: &EndpointId) -> Vec<MidiEvent> {
        self.channels
            .iter_mut()
            .enumerate()
            .filter_map(|(index, channel)| {
                channel.repress_at = None;
                if !channel.generated_pedal_down {
                    return None;
                }
                channel.output_pedal_down = false;
                channel.generated_pedal_down = false;
                channel.sustain_mask = 0;
                Some(MidiEvent::new(
                    source.clone(),
                    channel.last_timestamp_micros,
                    vec![0xB0 | index as u8, 64, 0],
                ))
            })
            .collect()
    }
}

fn cc64(event: &MidiEvent, value: u8) -> MidiEvent {
    MidiEvent::new(
        event.source.clone(),
        event.timestamp_micros,
        vec![0xB0 | (event.data[0] & 0x0F), 64, value],
    )
}

fn track_note_on(channel: &mut SustainChannel, note: u8) {
    let index = usize::from(note);
    channel.held_notes[index] = channel.held_notes[index].saturating_add(1);
    if channel.held_notes[index] == 1 {
        channel.active_note_count += 1;
        let pc = usize::from(note % 12);
        channel.held_pitch_classes[pc] = channel.held_pitch_classes[pc].saturating_add(1);
    }
}

fn track_note_off(channel: &mut SustainChannel, note: u8) {
    let index = usize::from(note);
    if channel.held_notes[index] == 0 {
        return;
    }
    channel.held_notes[index] -= 1;
    if channel.held_notes[index] == 0 {
        channel.active_note_count = channel.active_note_count.saturating_sub(1);
        let pc = usize::from(note % 12);
        channel.held_pitch_classes[pc] = channel.held_pitch_classes[pc].saturating_sub(1);
    }
}

fn add_held_to_pool(channel: &mut SustainChannel) {
    for (pc, count) in channel.held_pitch_classes.iter().enumerate() {
        if *count > 0 {
            channel.sustain_mask |= 1 << pc;
        }
    }
}

fn has_dissonance(pc: u8, mask: u16, sensitivity: DissonanceSensitivity) -> bool {
    let semitone = mask & (1 << ((pc + 1) % 12)) != 0 || mask & (1 << ((pc + 11) % 12)) != 0;
    let tritone = mask & (1 << ((pc + 6) % 12)) != 0;
    match sensitivity {
        DissonanceSensitivity::Off => false,
        DissonanceSensitivity::Semitone => semitone,
        DissonanceSensitivity::Tritone => tritone,
        DissonanceSensitivity::Both => semitone || tritone,
    }
}
