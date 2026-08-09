use serde::{Deserialize, Serialize};

use crate::EndpointId;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MidiEvent {
    pub source: EndpointId,
    pub timestamp_micros: u64,
    pub data: Vec<u8>,
}

impl MidiEvent {
    pub fn new(source: EndpointId, timestamp_micros: u64, data: Vec<u8>) -> Self {
        Self {
            source,
            timestamp_micros,
            data,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutedEvent {
    pub target: EndpointId,
    pub timestamp_micros: u64,
    pub data: Vec<u8>,
}
