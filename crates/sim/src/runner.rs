use std::{fs, path::Path};

use anyhow::{Context as AnyhowContext, Result};
use mote_api::{
    messages::router,
    work::{control, guidance, navigation},
};
use serde::de::DeserializeOwned;

#[derive(Default)]
struct HarnessContext {
    guidance_context: guidance::Context,
    control_context: control::Context,
    navigation_context: navigation::Context,
    route: Option<router::Route>,
    control_mode_enabled: bool,
    position: Option<router::Position>,
    reference_frame: Option<router::ReferenceFrame>,
    telemetry_selection: router::TelemetrySelection,
    telemetry: Vec<router::Message>,
    positions: Vec<router::Position>,
    guidance: Vec<router::Guidance>,
    controls: Vec<router::Control>,
    motor_commands: Vec<router::MotorDrive>,
}

impl HarnessContext {
    fn with_default_reference_frame() -> Self {
        Self {
            reference_frame: Some(router::ReferenceFrame::default()),
            ..Self::default()
        }
    }

    fn handle_imu(&mut self, timestamp: units::Time, imu: router::Imu) {
        if self.telemetry_selection.is_send_imu() {
            self.telemetry.push(router::Message::Imu(imu.clone()));
        }

        let position = self.reference_frame.as_ref().and_then(|reference_frame| {
            self.navigation_context.update(
                timestamp,
                self.position.as_ref(),
                None,
                Some(imu),
                reference_frame,
            )
        });
        self.handle_position(position, timestamp);
    }

    fn handle_lidar_scan(&mut self, timestamp: units::Time, scan: router::Scan) {
        if self.telemetry_selection.is_send_lidar() {
            self.telemetry
                .push(router::Message::LidarScan(scan.clone()));
        }

        let position = self.reference_frame.as_ref().and_then(|reference_frame| {
            self.navigation_context.update(
                timestamp,
                self.position.as_ref(),
                Some(scan),
                None,
                reference_frame,
            )
        });
        self.handle_position(position, timestamp);
    }

    fn handle_position(&mut self, position: Option<router::Position>, timestamp: units::Time) {
        if let Some(position) = position {
            if self.telemetry_selection.is_send_navigation() {
                self.telemetry
                    .push(router::Message::Position(position.clone()));
            }
            self.position = Some(position.clone());
            self.positions.push(position);
        }
        self.handle_update(timestamp);
    }

    fn handle_update(&mut self, timestamp: units::Time) {
        if let (Some(route), Some(position)) = (self.route.as_ref(), self.position.as_ref()) {
            let guidance = self.guidance_context.update(timestamp, position, route);
            if let Some(guidance) = guidance {
                if self.telemetry_selection.is_send_guidance() {
                    self.telemetry
                        .push(router::Message::Guidance(guidance.clone()));
                }
                self.guidance.push(guidance.clone());

                if let Some(control) = self.control_context.update(timestamp, Some(guidance)) {
                    if self.telemetry_selection.is_send_control() {
                        self.telemetry
                            .push(router::Message::Control(control.clone()));
                    }
                    if self.control_mode_enabled {
                        self.motor_commands.push(control.motor_drive.clone());
                    }
                    self.controls.push(control);
                }
            }
        }
    }
}

enum SensorEvent {
    Imu(router::Imu),
    LidarScan(router::Scan),
}

impl SensorEvent {
    fn timestamp(&self) -> units::Time {
        match self {
            Self::Imu(imu) => imu.timestamp,
            Self::LidarScan(scan) => scan.timestamp,
        }
    }
}

/// Replays IMU and LiDAR Postcard inputs through the navigation work context.
///
/// Inputs may be a single serialized value, a serialized sequence, concatenated
/// Postcard values, or the length-prefixed `router::Message` telemetry format
/// written by the mission app.
pub fn navigation(lidar_path: &Path) -> Result<()> {
    let scans = read_sensor_samples(lidar_path, "LiDAR", |message| match message {
        router::Message::LidarScan(scan) => Some(scan),
        _ => None,
    })?;

    let mut events = Vec::with_capacity(scans.len());
    events.extend(scans.into_iter().map(SensorEvent::LidarScan));
    events.sort_by(|left, right| {
        left.timestamp()
            .partial_cmp(&right.timestamp())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let lidar_count = events.len();
    let mut context = HarnessContext::with_default_reference_frame();

    for event in events {
        match event {
            SensorEvent::LidarScan(scan) => {
                let timestamp = scan.timestamp;
                context.handle_lidar_scan(timestamp, scan);
            }
            _ => (),
        }
    }

    println!("Replayed {lidar_count} LiDAR scans.");
    println!(
        "Harness outputs: {} positions, {} guidance updates, {} control updates, {} motor commands, {} telemetry messages.",
        context.positions.len(),
        context.guidance.len(),
        context.controls.len(),
        context.motor_commands.len(),
        context.telemetry.len(),
    );
    Ok(())
}

/// Converts a recorded LiDAR Postcard telemetry file to pretty-printed JSON.
///
/// The input is expected to contain `router::Message::LidarScan` records, using
/// the length-prefixed format written by the mission app or another supported
/// Postcard representation. When no output path is supplied, the input extension
/// is replaced with `.json`.
pub fn convert_lidar_to_json(input_path: &Path, output_path: Option<&Path>) -> Result<()> {
    let scans = read_sensor_samples(input_path, "LiDAR", |message| match message {
        router::Message::LidarScan(scan) => Some(scan),
        _ => None,
    })?;
    let points: Vec<_> = scans
        .into_iter()
        .flat_map(|scan| scan.point_cloud.into_iter())
        .collect();
    let output_path = output_path.map(Path::to_path_buf).unwrap_or_else(|| {
        let mut path = input_path.to_path_buf();
        path.set_extension("json");
        path
    });

    let mut json =
        serde_json::to_vec_pretty(&points).context("could not encode LiDAR points as JSON")?;
    json.push(b'\n');
    fs::write(&output_path, json)
        .with_context(|| format!("could not write JSON output {}", output_path.display()))?;
    println!(
        "Converted {} LiDAR points from {} to {}.",
        points.len(),
        input_path.display(),
        output_path.display(),
    );
    Ok(())
}

/// Runs the guidance work context once with the supplied route and position.
pub fn guidance(
    timestamp: units::Time,
    position: &router::Position,
    route: &router::Route,
) -> Option<router::Guidance> {
    guidance::Context::default().update(timestamp, position, route)
}

/// Runs the control work context once with the supplied guidance, if available.
pub fn control(
    timestamp: units::Time,
    guidance: Option<router::Guidance>,
) -> Option<router::Control> {
    control::Context::default().update(timestamp, guidance)
}

fn read_sensor_samples<T, F>(path: &Path, sensor: &str, extract: F) -> Result<Vec<T>>
where
    T: DeserializeOwned,
    F: Fn(router::Message) -> Option<T>,
{
    let bytes = fs::read(path)
        .with_context(|| format!("could not read {sensor} input {}", path.display()))?;

    decode_sensor_samples(&bytes, &extract).with_context(|| {
        format!(
            "{} input {} is not valid Postcard sensor data",
            sensor,
            path.display()
        )
    })
}

fn decode_sensor_samples<T, F>(bytes: &[u8], extract: &F) -> Option<Vec<T>>
where
    T: DeserializeOwned,
    F: Fn(router::Message) -> Option<T>,
{
    if let Some(samples) = decode_framed_messages(bytes, extract) {
        return Some(samples);
    }

    if let Ok(samples) = postcard::from_bytes::<Vec<T>>(bytes) {
        return Some(samples);
    }

    if let Some(messages) = decode_message_stream(bytes) {
        let samples: Vec<_> = messages.into_iter().filter_map(extract).collect();
        if !samples.is_empty() {
            return Some(samples);
        }
    }

    decode_value_stream(bytes)
}

fn decode_framed_messages<T, F>(bytes: &[u8], extract: &F) -> Option<Vec<T>>
where
    F: Fn(router::Message) -> Option<T>,
{
    let mut offset = 0;
    let mut samples = Vec::new();
    let mut frame_count = 0;

    while offset < bytes.len() {
        let length_bytes: [u8; 4] = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
        let length = u32::from_le_bytes(length_bytes) as usize;
        let frame_start = offset.checked_add(4)?;
        let frame_end = frame_start.checked_add(length)?;
        let frame = bytes.get(frame_start..frame_end)?;
        let message = postcard::from_bytes::<router::Message>(frame).ok()?;
        if let Some(sample) = extract(message) {
            samples.push(sample);
        }
        offset = frame_end;
        frame_count += 1;
    }

    (frame_count > 0 && !samples.is_empty()).then_some(samples)
}

fn decode_message_stream(bytes: &[u8]) -> Option<Vec<router::Message>> {
    let mut remaining = bytes;
    let mut messages = Vec::new();
    while !remaining.is_empty() {
        let (message, rest) = postcard::take_from_bytes::<router::Message>(remaining).ok()?;
        if rest.len() >= remaining.len() {
            return None;
        }
        messages.push(message);
        remaining = rest;
    }
    (!messages.is_empty()).then_some(messages)
}

fn decode_value_stream<T: DeserializeOwned>(bytes: &[u8]) -> Option<Vec<T>> {
    let mut remaining = bytes;
    let mut values = Vec::new();
    while !remaining.is_empty() {
        let (value, rest) = postcard::take_from_bytes::<T>(remaining).ok()?;
        if rest.len() >= remaining.len() {
            return None;
        }
        values.push(value);
        remaining = rest;
    }
    (!values.is_empty()).then_some(values)
}
