use std::{
    collections::BTreeMap,
    net::{Ipv4Addr, SocketAddr, UdpSocket},
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use mdns_sd::{DaemonEvent, Receiver as MdnsReceiver, ServiceDaemon, ServiceEvent, ServiceInfo};
use mote_api::{
    MoteLink,
    messages::{host_to_mote, mote_to_host, pubsub},
};

const SERVICE_TYPE: &str = "_mote-api._udp.local.";
const PING_INTERVAL: Duration = Duration::from_secs(1);
const LIVENESS_TIMEOUT: Duration = Duration::from_secs(4);
const WORKER_TICK: Duration = Duration::from_millis(25);
const SNAPSHOT_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone, Default)]
pub(crate) struct LatestTelemetry {
    pub guidance: Option<pubsub::Guidance>,
    pub guidance_received_at: Option<Instant>,
    pub position: Option<pubsub::Position>,
    pub position_received_at: Option<Instant>,
    pub control: Option<pubsub::Control>,
    pub control_received_at: Option<Instant>,
    pub imu: Option<pubsub::Imu>,
    pub imu_received_at: Option<Instant>,
    pub lidar_scan: Option<pubsub::Scan>,
    pub lidar_scan_received_at: Option<Instant>,
}

impl LatestTelemetry {
    pub(crate) fn update_pubsub(&mut self, message: pubsub::Message) -> bool {
        match message {
            pubsub::Message::Guidance(value) => {
                self.guidance = Some(value);
                self.guidance_received_at = Some(Instant::now());
            }
            pubsub::Message::Position(value) => {
                self.position = Some(value);
                self.position_received_at = Some(Instant::now());
            }
            pubsub::Message::Control(value) => {
                self.control = Some(value);
                self.control_received_at = Some(Instant::now());
            }
            pubsub::Message::Imu(value) => {
                self.imu = Some(value);
                self.imu_received_at = Some(Instant::now());
            }
            pubsub::Message::LidarScan(value) => {
                self.lidar_scan = Some(value);
                self.lidar_scan_received_at = Some(Instant::now());
            }
            _ => return false,
        }
        true
    }
}

#[derive(Clone, Debug)]
pub(crate) struct DiscoveredDevice {
    pub fullname: String,
    pub uid: String,
    pub hostname: String,
    pub endpoint: Option<SocketAddr>,
    pub status: String,
}

#[derive(Clone, Debug)]
pub(crate) enum ConnectionStatus {
    Disconnected,
    Connecting {
        uid: String,
        endpoint: SocketAddr,
    },
    Connected {
        uid: String,
        endpoint: SocketAddr,
        last_seen_ago_ms: u128,
    },
    Error(String),
}

#[derive(Clone)]
pub(crate) struct WorkerSnapshot {
    pub devices: Vec<DiscoveredDevice>,
    pub discovery_status: String,
    pub connection: ConnectionStatus,
    pub incoming_messages: u64,
    pub malformed_packets: u64,
    pub version_mismatches: u64,
    pub last_protocol_error: Option<String>,
    pub latest_telemetry: LatestTelemetry,
}

pub(crate) struct TransportWorker {
    commands: SyncSender<WorkerRequest>,
    events: Receiver<WorkerSnapshot>,
    thread: Option<JoinHandle<()>>,
}

impl TransportWorker {
    pub(crate) fn start() -> Self {
        let (command_sender, command_receiver) = mpsc::sync_channel(16);
        let (event_sender, event_receiver) = mpsc::sync_channel(8);
        let worker_thread = thread::Builder::new()
            .name("mote-transport".to_owned())
            .spawn(move || run_worker(command_receiver, event_sender))
            .expect("failed to start Mote transport worker");

        Self {
            commands: command_sender,
            events: event_receiver,
            thread: Some(worker_thread),
        }
    }

    pub(crate) fn command(&self, command: WorkerCommand) -> Result<(), String> {
        self.send_request(WorkerRequest::Command(command))
    }

    pub(crate) fn refresh(&self) -> Result<(), String> {
        self.send_request(WorkerRequest::Refresh)
    }

    pub(crate) fn connect(&self, fullname: String) -> Result<(), String> {
        self.send_request(WorkerRequest::Connect(fullname))
    }

    pub(crate) fn disconnect(&self) -> Result<(), String> {
        self.send_request(WorkerRequest::Disconnect)
    }

    fn send_request(&self, request: WorkerRequest) -> Result<(), String> {
        self.commands
            .try_send(request)
            .map_err(|error| match error {
                TrySendError::Full(_) => "Transport worker is busy; try again shortly".to_owned(),
                TrySendError::Disconnected(_) => "Transport worker is not available".to_owned(),
            })
    }

    pub(crate) fn try_snapshot(&self) -> Result<WorkerSnapshot, TryRecvError> {
        self.events.try_recv()
    }

    pub(crate) fn shutdown(&mut self) {
        if let Some(worker_thread) = self.thread.take() {
            let _ = self.commands.send(WorkerRequest::Shutdown);
            let _ = worker_thread.join();
        }
    }
}

impl Drop for TransportWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub(crate) enum WorkerCommand {
    HostToMote(host_to_mote::Message),
    PubSub(pubsub::Message),
}

enum WorkerRequest {
    Command(WorkerCommand),
    Refresh,
    Connect(String),
    Disconnect,
    Shutdown,
}

struct Discovery {
    daemon: ServiceDaemon,
    services: MdnsReceiver<ServiceEvent>,
    monitor: MdnsReceiver<DaemonEvent>,
}

struct ActiveSession {
    socket: UdpSocket,
    link: MoteLink,
    fullname: String,
    uid: String,
    endpoint: SocketAddr,
    connected: bool,
    last_seen: Instant,
    last_pong: Instant,
    last_ping: Instant,
}

struct WorkerState {
    devices: BTreeMap<String, DiscoveredDevice>,
    discovery_status: String,
    connection: ConnectionStatus,
    incoming_messages: u64,
    malformed_packets: u64,
    version_mismatches: u64,
    last_protocol_error: Option<String>,
    latest_telemetry: LatestTelemetry,
}

impl WorkerState {
    fn snapshot(&self, session: Option<&ActiveSession>) -> WorkerSnapshot {
        let connection = match session {
            Some(session) if session.connected => ConnectionStatus::Connected {
                uid: session.uid.clone(),
                endpoint: session.endpoint,
                last_seen_ago_ms: session.last_seen.elapsed().as_millis(),
            },
            Some(session) => ConnectionStatus::Connecting {
                uid: session.uid.clone(),
                endpoint: session.endpoint,
            },
            None => self.connection.clone(),
        };

        WorkerSnapshot {
            devices: self.devices.values().cloned().collect(),
            discovery_status: self.discovery_status.clone(),
            connection,
            incoming_messages: self.incoming_messages,
            malformed_packets: self.malformed_packets,
            version_mismatches: self.version_mismatches,
            last_protocol_error: self.last_protocol_error.clone(),
            latest_telemetry: self.latest_telemetry.clone(),
        }
    }
}

fn run_worker(commands: Receiver<WorkerRequest>, events: SyncSender<WorkerSnapshot>) {
    let mut state = WorkerState {
        devices: BTreeMap::new(),
        discovery_status: "Starting mDNS discovery…".to_owned(),
        connection: ConnectionStatus::Disconnected,
        incoming_messages: 0,
        malformed_packets: 0,
        version_mismatches: 0,
        last_protocol_error: None,
        latest_telemetry: LatestTelemetry::default(),
    };
    let mut discovery = match start_discovery() {
        Ok(discovery) => {
            state.discovery_status = format!("Searching for {SERVICE_TYPE}");
            Some(discovery)
        }
        Err(error) => {
            state.discovery_status = format!("Discovery error: {error}");
            None
        }
    };
    let mut session: Option<ActiveSession> = None;
    let mut dirty = true;
    let mut next_snapshot = Instant::now();

    loop {
        let mut shutting_down = false;
        loop {
            match commands.try_recv() {
                Ok(WorkerRequest::Refresh) => {
                    match discovery.as_mut() {
                        Some(current) => {
                            let refreshed = current
                                .daemon
                                .stop_browse(SERVICE_TYPE)
                                .and_then(|()| current.daemon.browse(SERVICE_TYPE));
                            match refreshed {
                                Ok(services) => {
                                    current.services = services;
                                    state.discovery_status =
                                        format!("Searching for {SERVICE_TYPE}");
                                }
                                Err(error) => {
                                    state.discovery_status =
                                        format!("Discovery refresh failed: {error}");
                                }
                            }
                        }
                        None => match start_discovery() {
                            Ok(new_discovery) => {
                                discovery = Some(new_discovery);
                                state.discovery_status = format!("Searching for {SERVICE_TYPE}");
                            }
                            Err(error) => {
                                state.discovery_status = format!("Discovery error: {error}");
                            }
                        },
                    }
                    dirty = true;
                }
                Ok(WorkerRequest::Connect(fullname)) => {
                    close_session(&mut session);
                    state.last_protocol_error = None;
                    state.latest_telemetry = LatestTelemetry::default();
                    match state.devices.get(&fullname).cloned() {
                        Some(device) => match device.endpoint {
                            Some(endpoint) => match connect_device(device, endpoint) {
                                Ok(new_session) => {
                                    state.connection = ConnectionStatus::Disconnected;
                                    session = Some(new_session);
                                }
                                Err(error) => {
                                    state.connection = ConnectionStatus::Error(error);
                                    session = None;
                                }
                            },
                            None => {
                                state.connection = ConnectionStatus::Error(format!(
                                    "{} has no resolved IPv4 address and cannot be connected",
                                    device.uid
                                ));
                            }
                        },
                        None => {
                            state.connection = ConnectionStatus::Error(
                                "The selected Mote is no longer in the discovery list".to_owned(),
                            );
                        }
                    }
                    dirty = true;
                }
                Ok(WorkerRequest::Disconnect) => {
                    let stop_error = close_session(&mut session);
                    state.connection = match stop_error {
                        Some(error) => ConnectionStatus::Error(format!(
                            "Disconnected, but the safe-stop command failed: {error}"
                        )),
                        None => ConnectionStatus::Disconnected,
                    };
                    dirty = true;
                }
                Ok(WorkerRequest::Command(WorkerCommand::HostToMote(message))) => {
                    if let Some(error) = send_active_message(&mut session, message) {
                        state.connection = ConnectionStatus::Error(error);
                    }
                    dirty = true;
                }
                Ok(WorkerRequest::Command(WorkerCommand::PubSub(message))) => {
                    if let Some(error) =
                        send_active_message(&mut session, host_to_mote::Message::PubSub(message))
                    {
                        state.connection = ConnectionStatus::Error(error);
                    }
                    dirty = true;
                }
                Ok(WorkerRequest::Shutdown) => {
                    shutting_down = true;
                    break;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    shutting_down = true;
                    break;
                }
            }
        }

        if shutting_down {
            close_session(&mut session);
            if let Some(discovery) = discovery.take() {
                shutdown_daemon(&discovery.daemon);
            }
            break;
        }

        if let Some(discovery) = discovery.as_mut() {
            while let Ok(event) = discovery.services.try_recv() {
                handle_service_event(event, &mut state, &mut session);
                dirty = true;
            }
            while let Ok(event) = discovery.monitor.try_recv() {
                if let DaemonEvent::Error(error) = event {
                    state.discovery_status = format!("mDNS discovery error: {error}");
                    dirty = true;
                }
            }
        }

        if let Some(active) = session.as_mut() {
            let now = Instant::now();
            match receive_messages(active, &mut state) {
                Ok(messages_changed_state) => {
                    if messages_changed_state {
                        dirty = true;
                    }
                }
                Err(error) => {
                    let uid = active.uid.clone();
                    let stop_error = close_session(&mut session);
                    state.connection = ConnectionStatus::Error(match stop_error {
                        Some(stop_error) => {
                            format!("{uid}: {error}; safe stop failed: {stop_error}")
                        }
                        None => format!("{uid}: {error}"),
                    });
                    dirty = true;
                }
            }

            if let Some(active) = session.as_mut() {
                if now.duration_since(active.last_pong) > LIVENESS_TIMEOUT {
                    let uid = active.uid.clone();
                    let stop_error = close_session(&mut session);
                    state.connection = ConnectionStatus::Error(match stop_error {
                        Some(stop_error) => format!(
                            "No response from {uid} for {} seconds; safe stop failed: {stop_error}",
                            LIVENESS_TIMEOUT.as_secs()
                        ),
                        None => format!(
                            "No response from {uid} for {} seconds",
                            LIVENESS_TIMEOUT.as_secs()
                        ),
                    });
                    dirty = true;
                } else if let Some(active) = session.as_mut()
                    && now.duration_since(active.last_ping) >= PING_INTERVAL
                {
                    match send_message(
                        &mut active.link,
                        &active.socket,
                        host_to_mote::Message::Ping,
                    ) {
                        Ok(()) => active.last_ping = now,
                        Err(error) => {
                            let uid = active.uid.clone();
                            let stop_error = close_session(&mut session);
                            state.connection = ConnectionStatus::Error(match stop_error {
                                Some(stop_error) => {
                                    format!(
                                        "{uid}: Ping failed: {error}; safe stop failed: {stop_error}"
                                    )
                                }
                                None => format!("{uid}: Ping failed: {error}"),
                            });
                            dirty = true;
                        }
                    }
                }
            }
        }

        let now = Instant::now();
        if dirty || now >= next_snapshot {
            match events.try_send(state.snapshot(session.as_ref())) {
                Ok(()) => dirty = false,
                Err(TrySendError::Full(_)) => dirty = true,
                Err(TrySendError::Disconnected(_)) => break,
            }
            next_snapshot = now + SNAPSHOT_INTERVAL;
        }

        thread::sleep(WORKER_TICK);
    }
}

fn start_discovery() -> Result<Discovery, String> {
    let daemon = ServiceDaemon::new().map_err(|error| error.to_string())?;
    let monitor = match daemon.monitor() {
        Ok(monitor) => monitor,
        Err(error) => {
            shutdown_daemon(&daemon);
            return Err(error.to_string());
        }
    };
    match daemon.browse(SERVICE_TYPE) {
        Ok(services) => Ok(Discovery {
            daemon,
            services,
            monitor,
        }),
        Err(error) => {
            shutdown_daemon(&daemon);
            Err(error.to_string())
        }
    }
}

fn shutdown_daemon(daemon: &ServiceDaemon) {
    if let Ok(status) = daemon.shutdown() {
        let _ = status.recv_timeout(Duration::from_secs(1));
    }
}

fn handle_service_event(
    event: ServiceEvent,
    state: &mut WorkerState,
    session: &mut Option<ActiveSession>,
) {
    match event {
        ServiceEvent::ServiceFound(_, fullname) => {
            state
                .devices
                .entry(fullname.clone())
                .or_insert_with(|| DiscoveredDevice {
                    uid: uid_from_fullname(&fullname),
                    fullname,
                    hostname: "Resolving hostname…".to_owned(),
                    endpoint: None,
                    status: "Resolving service details".to_owned(),
                });
            state.discovery_status = format!("Resolving {} device(s)", state.devices.len());
        }
        ServiceEvent::ServiceResolved(service) => {
            let device = device_from_service(&service);
            state.devices.insert(device.fullname.clone(), device);
            state.discovery_status = format!("Found {} Mote device(s)", state.devices.len());
        }
        ServiceEvent::ServiceRemoved(_, fullname) => {
            state.devices.remove(&fullname);
            if session
                .as_ref()
                .is_some_and(|active| active.fullname == fullname)
            {
                let uid = session
                    .as_ref()
                    .map_or_else(String::new, |active| active.uid.clone());
                let stop_error = close_session(session);
                state.connection = ConnectionStatus::Error(match stop_error {
                    Some(error) => format!(
                        "{uid} stopped advertising and was disconnected; safe stop failed: {error}"
                    ),
                    None => format!("{uid} stopped advertising and was disconnected"),
                });
            }
            state.discovery_status = if state.devices.is_empty() {
                format!("Searching for {SERVICE_TYPE}; no devices currently found")
            } else {
                format!("Found {} Mote device(s)", state.devices.len())
            };
        }
        ServiceEvent::SearchStarted(_) | ServiceEvent::SearchStopped(_) => {}
    }
}

fn device_from_service(service: &ServiceInfo) -> DiscoveredDevice {
    let address = service
        .get_addresses_v4()
        .into_iter()
        .min()
        .map(|ip: &Ipv4Addr| SocketAddr::new((*ip).into(), service.get_port()));
    let status = if address.is_some() {
        "Available".to_owned()
    } else {
        "Resolved, but no IPv4 address was advertised".to_owned()
    };

    DiscoveredDevice {
        fullname: service.get_fullname().to_owned(),
        uid: uid_from_fullname(service.get_fullname()),
        hostname: service.get_hostname().to_owned(),
        endpoint: address,
        status,
    }
}

fn uid_from_fullname(fullname: &str) -> String {
    fullname
        .strip_suffix("._mote-api._udp.local.")
        .or_else(|| fullname.strip_suffix("._mote-api._udp.local"))
        .unwrap_or(fullname)
        .to_owned()
}

fn connect_device(device: DiscoveredDevice, endpoint: SocketAddr) -> Result<ActiveSession, String> {
    let bind_address = match endpoint {
        SocketAddr::V4(_) => SocketAddr::from(([0, 0, 0, 0], 0)),
        SocketAddr::V6(_) => SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 0], 0)),
    };
    let socket =
        UdpSocket::bind(bind_address).map_err(|error| format!("UDP bind failed: {error}"))?;
    socket
        .connect(endpoint)
        .map_err(|error| format!("UDP connect to {endpoint} failed: {error}"))?;
    socket
        .set_nonblocking(true)
        .map_err(|error| format!("Could not configure UDP socket: {error}"))?;

    let now = Instant::now();
    let mut link = MoteLink::new();
    send_message(&mut link, &socket, host_to_mote::Message::Ping)
        .map_err(|error| format!("Could not send initial protocol Ping: {error}"))?;

    Ok(ActiveSession {
        socket,
        link,
        fullname: device.fullname,
        uid: device.uid,
        endpoint,
        connected: false,
        last_seen: now,
        last_pong: now,
        last_ping: now,
    })
}

fn send_message(
    link: &mut MoteLink,
    socket: &UdpSocket,
    message: host_to_mote::Message,
) -> Result<(), String> {
    link.send(message).map_err(|error| error.to_string())?;
    while let Some(packet) = link.poll_transmit() {
        let sent = socket
            .send(&packet)
            .map_err(|error| format!("UDP send failed: {error}"))?;
        if sent != packet.len() {
            return Err(format!("UDP sent {sent} of {} bytes", packet.len()));
        }
    }
    Ok(())
}

fn receive_messages(session: &mut ActiveSession, state: &mut WorkerState) -> Result<bool, String> {
    let mut buffer = [0_u8; 4096];
    let mut state_changed = false;

    loop {
        match session.socket.recv(&mut buffer) {
            Ok(length) => {
                session.link.handle_receive(&buffer[..length]);
                loop {
                    match session.link.poll_receive() {
                        Ok(Some(message)) => {
                            state.incoming_messages += 1;
                            session.last_seen = Instant::now();
                            match message {
                                mote_to_host::Message::Pong => {
                                    session.last_pong = Instant::now();
                                    if !session.connected {
                                        session.connected = true;
                                        state_changed = true;
                                    }
                                }
                                mote_to_host::Message::Scan(scan) => {
                                    let scan = pubsub_lidar_scan(&scan);
                                    state_changed |= state
                                        .latest_telemetry
                                        .update_pubsub(pubsub::Message::LidarScan(scan));
                                }
                                mote_to_host::Message::PubSub(message) => {
                                    state_changed |= state.latest_telemetry.update_pubsub(message);
                                }
                                mote_to_host::Message::Ping => {
                                    send_message(
                                        &mut session.link,
                                        &session.socket,
                                        host_to_mote::Message::Pong,
                                    )?;
                                }
                                _ => {}
                            }
                        }
                        Ok(None) => break,
                        Err(error) => {
                            if matches!(&error, mote_api::Error::VersionMismatch { .. }) {
                                state.version_mismatches += 1;
                            } else {
                                state.malformed_packets += 1;
                            }
                            state.last_protocol_error = Some(error.to_string());
                        }
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(error) => return Err(format!("UDP receive failed: {error}")),
        }
    }

    Ok(state_changed)
}

fn pubsub_lidar_scan(scan: &[mote_to_host::Point]) -> pubsub::Scan {
    let mut converted = pubsub::Scan::new();
    for reading in scan {
        let point = pubsub::ScanPoint {
            timestamp: units::Time::new(0.0),
            quality: reading.quality,
            angle: units::PlaneAngle::new(reading.angle_rad),
            distance: units::Length::new(reading.distance_mm / 1_000.0),
        };
        if converted.push(point).is_err() {
            break;
        }
    }
    converted
}

fn send_active_message(
    session: &mut Option<ActiveSession>,
    message: host_to_mote::Message,
) -> Option<String> {
    let (uid, result) = {
        let active = session.as_mut().filter(|active| active.connected)?;
        (
            active.uid.clone(),
            send_message(&mut active.link, &active.socket, message),
        )
    };

    result.err().map(|error| {
        let stop_error = close_session(session);
        match stop_error {
            Some(stop_error) => {
                format!("{uid}: command failed: {error}; safe stop failed: {stop_error}")
            }
            None => format!("{uid}: command failed: {error}"),
        }
    })
}

fn close_session(session: &mut Option<ActiveSession>) -> Option<String> {
    let mut active = session.take()?;
    if active.connected && active.last_pong.elapsed() <= LIVENESS_TIMEOUT {
        send_message(
            &mut active.link,
            &active.socket,
            host_to_mote::Message::SetDriveBaseVelocity(host_to_mote::SetDriveBaseVelocity {
                left_velocity_rad_per_s: 0.0,
                right_velocity_rad_per_s: 0.0,
            }),
        )
        .err()
    } else {
        None
    }
}
