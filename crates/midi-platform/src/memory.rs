use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crossbeam_channel::{Receiver, Sender, bounded};
use virtual_midi_core::{Config, EndpointId, MidiEvent};

use crate::{EndpointInfo, MidiBackend, PlatformError, PlatformStatus};

#[derive(Clone)]
pub struct MemoryBackend {
    incoming_tx: Sender<MidiEvent>,
    incoming_rx: Receiver<MidiEvent>,
    endpoints: Arc<Mutex<Vec<EndpointInfo>>>,
    sent: Arc<Mutex<HashMap<EndpointId, Vec<Vec<u8>>>>>,
    queue_overflows: Arc<AtomicU64>,
}

impl MemoryBackend {
    pub fn new(capacity: usize) -> Self {
        let (incoming_tx, incoming_rx) = bounded(capacity);
        Self {
            incoming_tx,
            incoming_rx,
            endpoints: Arc::new(Mutex::new(Vec::new())),
            sent: Arc::new(Mutex::new(HashMap::new())),
            queue_overflows: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn inject(&self, event: MidiEvent) -> bool {
        if self.incoming_tx.try_send(event).is_ok() {
            true
        } else {
            self.queue_overflows.fetch_add(1, Ordering::Relaxed);
            false
        }
    }

    pub fn sent_to(&self, target: &EndpointId) -> Vec<Vec<u8>> {
        self.sent
            .lock()
            .expect("memory backend lock")
            .get(target)
            .cloned()
            .unwrap_or_default()
    }

    pub fn set_endpoints(&mut self, endpoints: Vec<EndpointInfo>) {
        *self.endpoints.lock().expect("memory backend lock") = endpoints;
    }
}

impl MidiBackend for MemoryBackend {
    fn reconfigure(&mut self, _config: &Config) -> Result<Vec<EndpointInfo>, PlatformError> {
        Ok(self.endpoints.lock().expect("memory backend lock").clone())
    }

    fn endpoints(&self) -> Vec<EndpointInfo> {
        self.endpoints.lock().expect("memory backend lock").clone()
    }

    fn take_queue_overflows(&self) -> u64 {
        self.queue_overflows.swap(0, Ordering::Relaxed)
    }

    fn try_receive(&self) -> Option<MidiEvent> {
        self.incoming_rx.try_recv().ok()
    }

    fn send(&mut self, target: &EndpointId, data: &[u8]) -> Result<(), PlatformError> {
        self.sent
            .lock()
            .expect("memory backend lock")
            .entry(target.clone())
            .or_default()
            .push(data.to_vec());
        Ok(())
    }

    fn status(&self) -> PlatformStatus {
        PlatformStatus {
            platform: "memory".into(),
            runtime_available: true,
            detail: "In-memory test backend".into(),
        }
    }
}
