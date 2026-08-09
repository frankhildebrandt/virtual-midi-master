use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crossbeam_channel::{Receiver, Sender, bounded};
use midir::os::unix::{VirtualInput, VirtualOutput};
use midir::{Ignore, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use virtual_midi_core::{Config, EndpointDirection, EndpointId, EndpointKind, MidiEvent};

use crate::{EndpointInfo, MidiBackend, PlatformError, PlatformStatus};

pub struct MacOsBackend {
    incoming_tx: Sender<MidiEvent>,
    incoming_rx: Receiver<MidiEvent>,
    input_connections: Vec<MidiInputConnection<()>>,
    outputs: HashMap<EndpointId, MidiOutputConnection>,
    endpoints: Vec<EndpointInfo>,
    config: Config,
    queue_overflows: Arc<AtomicU64>,
}

impl MacOsBackend {
    pub fn new(queue_capacity: usize) -> Self {
        let (incoming_tx, incoming_rx) = bounded(queue_capacity);
        Self {
            incoming_tx,
            incoming_rx,
            input_connections: Vec::new(),
            outputs: HashMap::new(),
            endpoints: Vec::new(),
            config: Config::default(),
            queue_overflows: Arc::new(AtomicU64::new(0)),
        }
    }

    fn physical_signature(&self) -> Result<Vec<(u8, String)>, PlatformError> {
        let own_ports = self
            .config
            .virtual_devices
            .iter()
            .flat_map(|device| {
                [
                    format!("{} MIDI In", device.name),
                    format!("{} MIDI Out", device.name),
                ]
            })
            .collect::<std::collections::HashSet<_>>();
        let input = MidiInput::new("Virtual MIDI Master hotplug discovery")
            .map_err(|error| PlatformError::Initialization(format!("{error:?}")))?;
        let mut signature = input
            .ports()
            .iter()
            .filter_map(|port| input.port_name(port).ok().map(|name| (port.id(), name)))
            .filter(|(_, name)| !own_ports.contains(name))
            .map(|(id, name)| (0, format!("physical:input:{id}|{name}")))
            .collect::<Vec<_>>();
        let output = MidiOutput::new("Virtual MIDI Master hotplug discovery")
            .map_err(|error| PlatformError::Initialization(format!("{error:?}")))?;
        signature.extend(
            output
                .ports()
                .iter()
                .filter_map(|port| output.port_name(port).ok().map(|name| (port.id(), name)))
                .filter(|(_, name)| !own_ports.contains(name))
                .map(|(id, name)| (1, format!("physical:output:{id}|{name}"))),
        );
        Ok(signature)
    }

    fn connected_physical_signature(&self) -> Vec<(u8, String)> {
        self.endpoints
            .iter()
            .filter(|endpoint| endpoint.kind == EndpointKind::Physical)
            .map(|endpoint| {
                (
                    u8::from(endpoint.direction == EndpointDirection::Target),
                    format!("{}|{}", endpoint.id, endpoint.name),
                )
            })
            .collect()
    }

    fn connect_physical_inputs(&mut self) -> Result<(), PlatformError> {
        let probe = MidiInput::new("Virtual MIDI Master discovery")
            .map_err(|error| PlatformError::Initialization(format!("{error:?}")))?;
        let names = probe
            .ports()
            .iter()
            .filter_map(|port| probe.port_name(port).ok().map(|name| (port.id(), name)))
            .collect::<Vec<_>>();
        for (port_id, name) in names {
            let mut input = MidiInput::new("Virtual MIDI Master input")
                .map_err(|error| PlatformError::Initialization(format!("{error:?}")))?;
            input.ignore(Ignore::None);
            let ports = input.ports();
            let Some(port) = ports.iter().find(|port| port.id() == port_id) else {
                continue;
            };
            let id = EndpointId::new(format!("physical:input:{port_id}"));
            let callback_id = id.clone();
            let tx = self.incoming_tx.clone();
            let overflows = self.queue_overflows.clone();
            match input.connect(
                port,
                &format!("Virtual MIDI Master ← {name}"),
                move |timestamp, message, ()| {
                    if tx
                        .try_send(MidiEvent::new(
                            callback_id.clone(),
                            timestamp,
                            message.to_vec(),
                        ))
                        .is_err()
                    {
                        overflows.fetch_add(1, Ordering::Relaxed);
                    }
                },
                (),
            ) {
                Ok(connection) => {
                    self.input_connections.push(connection);
                    self.endpoints.push(EndpointInfo {
                        id,
                        name,
                        direction: EndpointDirection::Source,
                        kind: EndpointKind::Physical,
                        online: true,
                    });
                }
                Err(error) => tracing_error(&format!("could not open MIDI input: {error}")),
            }
        }
        Ok(())
    }

    fn connect_physical_outputs(&mut self) -> Result<(), PlatformError> {
        let probe = MidiOutput::new("Virtual MIDI Master discovery")
            .map_err(|error| PlatformError::Initialization(format!("{error:?}")))?;
        let names = probe
            .ports()
            .iter()
            .filter_map(|port| probe.port_name(port).ok().map(|name| (port.id(), name)))
            .collect::<Vec<_>>();
        for (port_id, name) in names {
            let output = MidiOutput::new("Virtual MIDI Master output")
                .map_err(|error| PlatformError::Initialization(format!("{error:?}")))?;
            let ports = output.ports();
            let Some(port) = ports.iter().find(|port| port.id() == port_id) else {
                continue;
            };
            let id = EndpointId::new(format!("physical:output:{port_id}"));
            match output.connect(port, &format!("Virtual MIDI Master → {name}")) {
                Ok(connection) => {
                    self.outputs.insert(id.clone(), connection);
                    self.endpoints.push(EndpointInfo {
                        id,
                        name,
                        direction: EndpointDirection::Target,
                        kind: EndpointKind::Physical,
                        online: true,
                    });
                }
                Err(error) => tracing_error(&format!("could not open MIDI output: {error}")),
            }
        }
        Ok(())
    }

    fn create_virtual_devices(&mut self, config: &Config) -> Result<(), PlatformError> {
        for device in config
            .virtual_devices
            .iter()
            .filter(|device| device.enabled)
        {
            let input_id = EndpointId::new(format!("virtual:{}:in", device.id));
            let output_id = EndpointId::new(format!("virtual:{}:out", device.id));

            let mut input = MidiInput::new("Virtual MIDI Master virtual input")
                .map_err(|error| PlatformError::Initialization(format!("{error:?}")))?;
            input.ignore(Ignore::None);
            let callback_id = input_id.clone();
            let tx = self.incoming_tx.clone();
            let overflows = self.queue_overflows.clone();
            let connection = input
                .create_virtual(
                    &format!("{} MIDI In", device.name),
                    move |timestamp, message, ()| {
                        if tx
                            .try_send(MidiEvent::new(
                                callback_id.clone(),
                                timestamp,
                                message.to_vec(),
                            ))
                            .is_err()
                        {
                            overflows.fetch_add(1, Ordering::Relaxed);
                        }
                    },
                    (),
                )
                .map_err(|error| PlatformError::Endpoint(error.to_string()))?;
            self.input_connections.push(connection);
            self.endpoints.push(EndpointInfo {
                id: input_id,
                name: format!("{} MIDI In", device.name),
                direction: EndpointDirection::Source,
                kind: EndpointKind::Virtual,
                online: true,
            });

            let output = MidiOutput::new("Virtual MIDI Master virtual output")
                .map_err(|error| PlatformError::Initialization(format!("{error:?}")))?;
            let connection = output
                .create_virtual(&format!("{} MIDI Out", device.name))
                .map_err(|error| PlatformError::Endpoint(error.to_string()))?;
            self.outputs.insert(output_id.clone(), connection);
            self.endpoints.push(EndpointInfo {
                id: output_id,
                name: format!("{} MIDI Out", device.name),
                direction: EndpointDirection::Target,
                kind: EndpointKind::Virtual,
                online: true,
            });
        }
        Ok(())
    }

    fn rebuild(&mut self, config: &Config) -> Result<Vec<EndpointInfo>, PlatformError> {
        self.input_connections.clear();
        self.outputs.clear();
        self.endpoints.clear();
        self.connect_physical_inputs()?;
        self.connect_physical_outputs()?;
        self.create_virtual_devices(config)?;
        self.config = config.clone();
        Ok(self.endpoints.clone())
    }
}

impl MidiBackend for MacOsBackend {
    fn reconfigure(&mut self, config: &Config) -> Result<Vec<EndpointInfo>, PlatformError> {
        if !self.endpoints.is_empty() && self.config.virtual_devices == config.virtual_devices {
            self.config = config.clone();
            return Ok(self.endpoints.clone());
        }
        self.rebuild(config)
    }

    fn refresh(&mut self) -> Result<Option<Vec<EndpointInfo>>, PlatformError> {
        if self.physical_signature()? == self.connected_physical_signature() {
            return Ok(None);
        }
        let config = self.config.clone();
        self.rebuild(&config).map(Some)
    }

    fn endpoints(&self) -> Vec<EndpointInfo> {
        self.endpoints.clone()
    }

    fn take_queue_overflows(&self) -> u64 {
        self.queue_overflows.swap(0, Ordering::Relaxed)
    }

    fn try_receive(&self) -> Option<MidiEvent> {
        self.incoming_rx.try_recv().ok()
    }

    fn send(&mut self, target: &EndpointId, data: &[u8]) -> Result<(), PlatformError> {
        self.outputs
            .get_mut(target)
            .ok_or_else(|| PlatformError::Endpoint(format!("offline target {target}")))?
            .send(data)
            .map_err(|error| PlatformError::Endpoint(error.to_string()))
    }

    fn status(&self) -> PlatformStatus {
        PlatformStatus {
            platform: "macos-coremidi".into(),
            runtime_available: true,
            detail: "CoreMIDI virtual endpoints are available".into(),
        }
    }
}

fn tracing_error(message: &str) {
    eprintln!("{message}");
}
