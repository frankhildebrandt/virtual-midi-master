use std::cell::RefCell;
use std::collections::HashMap;

use crate::effects::EffectRack;
use crate::{Config, ConfigError, EndpointId, MessageType, MidiEvent, RoutedEvent, TransformRule};

#[derive(Clone, Debug)]
struct CompiledRoute {
    target: EndpointId,
    rules: Vec<TransformRule>,
}

#[derive(Clone, Debug, Default)]
pub struct Router {
    routes: HashMap<EndpointId, Vec<CompiledRoute>>,
    effects: RefCell<EffectRack>,
}

impl Router {
    pub fn compile(config: &Config) -> Result<Self, ConfigError> {
        config.validate()?;
        let mut routes: HashMap<EndpointId, Vec<CompiledRoute>> = HashMap::new();
        for route in config.routes.iter().filter(|route| route.enabled) {
            routes
                .entry(route.source.clone())
                .or_default()
                .push(CompiledRoute {
                    target: route.target.clone(),
                    rules: route.rules.clone(),
                });
        }
        Ok(Self {
            routes,
            effects: RefCell::new(EffectRack::compile(config)),
        })
    }

    pub fn process(&self, event: MidiEvent) -> Vec<RoutedEvent> {
        let events = self.effects.borrow_mut().process(event);
        events
            .into_iter()
            .flat_map(|event| self.route(event))
            .collect()
    }

    pub fn poll_due(&self) -> Vec<RoutedEvent> {
        let events = self.effects.borrow_mut().poll_due();
        events
            .into_iter()
            .flat_map(|event| self.route(event))
            .collect()
    }

    pub fn release_generated_sustain(&self) -> Vec<RoutedEvent> {
        let events = self.effects.borrow_mut().release_generated_sustain();
        events
            .into_iter()
            .flat_map(|event| self.route(event))
            .collect()
    }

    fn route(&self, event: MidiEvent) -> Vec<RoutedEvent> {
        self.routes
            .get(&event.source)
            .into_iter()
            .flatten()
            .filter_map(|route| {
                let mut data = event.data.clone();
                for rule in &route.rules {
                    if !apply(rule, &mut data) {
                        return None;
                    }
                }
                Some(RoutedEvent {
                    target: route.target.clone(),
                    timestamp_micros: event.timestamp_micros,
                    data,
                })
            })
            .collect()
    }
}

fn apply(rule: &TransformRule, data: &mut [u8]) -> bool {
    match rule {
        TransformRule::DropMessageType { message_type } => !matches_type(data, *message_type),
        TransformRule::MapChannel { from, to } => {
            if let Some(status) = data
                .first_mut()
                .filter(|status| (0x80..=0xEF).contains(&**status))
            {
                let channel = *status & 0x0F;
                if from.is_none_or(|from| from == channel) {
                    *status = (*status & 0xF0) | *to;
                }
            }
            true
        }
        TransformRule::MapControlChange { from, to, channel } => {
            if data.len() >= 2 && data[0] & 0xF0 == 0xB0 && data[1] == *from {
                let event_channel = data[0] & 0x0F;
                if channel.is_none_or(|channel| channel == event_channel) {
                    data[1] = *to;
                }
            }
            true
        }
    }
}

fn matches_type(data: &[u8], message_type: MessageType) -> bool {
    let Some(status) = data.first().copied() else {
        return false;
    };
    match message_type {
        MessageType::PitchBend => status & 0xF0 == 0xE0,
        MessageType::PolyAftertouch => status & 0xF0 == 0xA0,
        MessageType::ChannelAftertouch => status & 0xF0 == 0xD0,
        MessageType::MidiClock => status == 0xF8,
        MessageType::SystemRealtime => (0xF9..=0xFF).contains(&status),
        MessageType::SysEx => status == 0xF0,
    }
}
