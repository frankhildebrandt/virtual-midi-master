use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

pub const CURRENT_CONFIG_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EndpointId(String);

impl EndpointId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EndpointId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointDirection {
    Source,
    Target,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointKind {
    Physical,
    Virtual,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointRef {
    pub id: EndpointId,
    pub direction: EndpointDirection,
    pub kind: EndpointKind,
    pub last_known_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VirtualDevice {
    pub id: Uuid,
    pub name: String,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    #[serde(default)]
    pub effects: Vec<MidiEffect>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScaleType {
    Major,
    NaturalMinor,
    HarmonicMinor,
    MelodicMinor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SustainMode {
    ChordGesture,
    Dissonance,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DissonanceSensitivity {
    Off,
    Semitone,
    Tritone,
    Both,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MidiEffect {
    ScaleHelper {
        #[serde(default = "enabled_by_default")]
        enabled: bool,
        root: u8,
        scale: ScaleType,
    },
    SustainHelper {
        #[serde(default = "enabled_by_default")]
        enabled: bool,
        mode: SustainMode,
        min_sustain_notes: u8,
        release_time_ms: u16,
        dissonance: DissonanceSensitivity,
        auto_sustain: bool,
    },
}

fn enabled_by_default() -> bool {
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    PitchBend,
    PolyAftertouch,
    ChannelAftertouch,
    MidiClock,
    SystemRealtime,
    SysEx,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransformRule {
    DropMessageType {
        message_type: MessageType,
    },
    MapChannel {
        #[serde(default)]
        from: Option<u8>,
        to: u8,
    },
    MapControlChange {
        from: u8,
        to: u8,
        #[serde(default)]
        channel: Option<u8>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    pub id: Uuid,
    pub source: EndpointId,
    pub target: EndpointId,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    #[serde(default)]
    pub rules: Vec<TransformRule>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "current_version")]
    pub version: u32,
    #[serde(default)]
    pub virtual_devices: Vec<VirtualDevice>,
    #[serde(default)]
    pub known_endpoints: Vec<EndpointRef>,
    #[serde(default)]
    pub routes: Vec<Route>,
    #[serde(default = "enabled_by_default")]
    pub autostart: bool,
}

const fn current_version() -> u32 {
    CURRENT_CONFIG_VERSION
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CURRENT_CONFIG_VERSION,
            virtual_devices: Vec::new(),
            known_endpoints: Vec::new(),
            routes: Vec::new(),
            autostart: true,
        }
    }
}

impl Config {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.version != CURRENT_CONFIG_VERSION {
            return Err(ConfigError::UnsupportedVersion(self.version));
        }

        let mut device_ids = HashSet::new();
        let mut device_names = HashSet::new();
        for device in &self.virtual_devices {
            if device.name.trim().is_empty() {
                return Err(ConfigError::EmptyVirtualDeviceName(device.id));
            }
            if !device_ids.insert(device.id) {
                return Err(ConfigError::DuplicateVirtualDevice(device.id));
            }
            if !device_names.insert(device.name.trim().to_lowercase()) {
                return Err(ConfigError::DuplicateVirtualDeviceName(device.name.clone()));
            }
            for effect in &device.effects {
                match effect {
                    MidiEffect::ScaleHelper { root, .. } if *root > 11 => {
                        return Err(ConfigError::InvalidMidiEffect(device.id));
                    }
                    MidiEffect::SustainHelper {
                        min_sustain_notes,
                        release_time_ms,
                        ..
                    } if !(1..=6).contains(min_sustain_notes)
                        || !(5..=250).contains(release_time_ms) =>
                    {
                        return Err(ConfigError::InvalidMidiEffect(device.id));
                    }
                    _ => {}
                }
            }
        }

        let mut endpoint_ids = HashSet::new();
        for endpoint in &self.known_endpoints {
            if endpoint.id.as_str().is_empty() {
                return Err(ConfigError::EmptyKnownEndpoint);
            }
            if !endpoint_ids.insert(&endpoint.id) {
                return Err(ConfigError::DuplicateKnownEndpoint(endpoint.id.to_string()));
            }
        }

        let mut route_ids = HashSet::new();
        let mut graph: HashMap<&EndpointId, Vec<&EndpointId>> = HashMap::new();
        for route in &self.routes {
            if !route_ids.insert(route.id) {
                return Err(ConfigError::DuplicateRoute(route.id));
            }
            validate_rules(route)?;
            if !route.enabled {
                continue;
            }
            if route.source.as_str().is_empty() || route.target.as_str().is_empty() {
                return Err(ConfigError::EmptyEndpoint(route.id));
            }
            if route.source == route.target {
                return Err(ConfigError::RoutingCycle(route.source.to_string()));
            }
            graph.entry(&route.source).or_default().push(&route.target);
        }

        if let Some(endpoint) = find_cycle(&graph) {
            return Err(ConfigError::RoutingCycle(endpoint.to_string()));
        }
        Ok(())
    }
}

fn validate_rules(route: &Route) -> Result<(), ConfigError> {
    for rule in &route.rules {
        match rule {
            TransformRule::MapChannel { from, to } => {
                if *to > 15 || from.is_some_and(|channel| channel > 15) {
                    return Err(ConfigError::InvalidChannel(route.id));
                }
            }
            TransformRule::MapControlChange { from, to, channel } => {
                if *from > 127 || *to > 127 {
                    return Err(ConfigError::InvalidControlChange(route.id));
                }
                if channel.is_some_and(|channel| channel > 15) {
                    return Err(ConfigError::InvalidChannel(route.id));
                }
            }
            TransformRule::DropMessageType { .. } => {}
        }
    }
    Ok(())
}

fn find_cycle<'a>(graph: &HashMap<&'a EndpointId, Vec<&'a EndpointId>>) -> Option<&'a EndpointId> {
    fn visit<'a>(
        node: &'a EndpointId,
        graph: &HashMap<&'a EndpointId, Vec<&'a EndpointId>>,
        visiting: &mut HashSet<&'a EndpointId>,
        visited: &mut HashSet<&'a EndpointId>,
    ) -> Option<&'a EndpointId> {
        if visiting.contains(node) {
            return Some(node);
        }
        if visited.contains(node) {
            return None;
        }
        visiting.insert(node);
        for next in graph.get(node).into_iter().flatten() {
            if let Some(cycle) = visit(next, graph, visiting, visited) {
                return Some(cycle);
            }
        }
        visiting.remove(node);
        visited.insert(node);
        None
    }

    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    for node in graph.keys() {
        if let Some(cycle) = visit(node, graph, &mut visiting, &mut visited) {
            return Some(cycle);
        }
    }
    None
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("unsupported configuration version {0}")]
    UnsupportedVersion(u32),
    #[error("virtual device {0} has an empty name")]
    EmptyVirtualDeviceName(Uuid),
    #[error("duplicate virtual device id {0}")]
    DuplicateVirtualDevice(Uuid),
    #[error("duplicate virtual device name {0}")]
    DuplicateVirtualDeviceName(String),
    #[error("virtual device {0} has invalid MIDI effect settings")]
    InvalidMidiEffect(Uuid),
    #[error("duplicate route id {0}")]
    DuplicateRoute(Uuid),
    #[error("route {0} has an empty endpoint")]
    EmptyEndpoint(Uuid),
    #[error("routing cycle involving {0}")]
    RoutingCycle(String),
    #[error("route {0} contains an invalid MIDI channel")]
    InvalidChannel(Uuid),
    #[error("route {0} contains an invalid control-change number")]
    InvalidControlChange(Uuid),
    #[error("known endpoint id cannot be empty")]
    EmptyKnownEndpoint,
    #[error("duplicate known endpoint id {0}")]
    DuplicateKnownEndpoint(String),
}
