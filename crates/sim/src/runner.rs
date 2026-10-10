use std::{
    fs,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context as AnyhowContext, Result};
use mote_api::{
    messages::router,
    work::{control, guidance, navigation},
};
use serde::{Serialize, de::DeserializeOwned};

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
    navigation_reports: Vec<navigation::NavigationReport>,
}

impl HarnessContext {
    fn with_default_reference_frame() -> Self {
        Self {
            reference_frame: Some(canonical_reference_frame()),
            ..Self::default()
        }
    }

    fn collect_navigation_reports(&mut self) {
        while let Some(report) = self.navigation_context.take_navigation_report() {
            self.navigation_reports.push(report);
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
        self.collect_navigation_reports();
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
        self.collect_navigation_reports();
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

#[derive(Clone, Copy)]
enum InputFormat {
    Binary,
    Json,
}

impl InputFormat {
    fn name(self) -> &'static str {
        match self {
            Self::Binary => "binary",
            Self::Json => "json",
        }
    }
}

struct NavigationInput {
    format: InputFormat,
    events: Vec<SensorEvent>,
    scan_count: usize,
    point_count: usize,
}

#[derive(Serialize)]
struct DumpCorner {
    kind: &'static str,
    vertex: [f32; 2],
    range_m: f32,
    bearing_rad: f32,
    interior_angle_rad: f32,
    bisector_rad: f32,
    score: f32,
}

#[derive(Serialize)]
struct DumpPose {
    timestamp_ms: f32,
    latitude_deg: f32,
    longitude_deg: f32,
    heading_deg: f32,
}

#[derive(Serialize)]
struct DumpLine {
    input: String,
    input_format: &'static str,
    timestamp_ms: f32,
    filled_bins: u16,
    corners: Vec<DumpCorner>,
    pose: Option<DumpPose>,
}

/// Replay one recording through the integrated navigation context.
pub fn navigation(input_path: &Path) -> Result<()> {
    navigation_paths(&[input_path.to_path_buf()], None)
}

/// Replay one or more binary/JSON recordings and optionally write JSON lines
/// describing every completed revolution.
pub fn navigation_paths(input_paths: &[PathBuf], dump_path: Option<&Path>) -> Result<()> {
    if input_paths.is_empty() {
        anyhow::bail!("at least one navigation input is required");
    }

    let mut dump_lines = Vec::new();
    for input_path in input_paths {
        let (input, context) = replay_navigation(input_path)?;
        print_navigation_summary(input_path, &input, &context);
        if dump_path.is_some() {
            dump_lines.extend(dump_lines_for(input_path, &input, &context));
        }
    }

    if let Some(dump_path) = dump_path {
        let file = fs::File::create(dump_path)
            .with_context(|| format!("could not create dump {}", dump_path.display()))?;
        let mut writer = BufWriter::new(file);
        let dump_count = dump_lines.len();
        for line in &dump_lines {
            serde_json::to_writer(&mut writer, &line)
                .context("could not encode navigation dump line")?;
            writer
                .write_all(b"\n")
                .context("could not write navigation dump")?;
        }
        writer
            .flush()
            .with_context(|| format!("could not flush dump {}", dump_path.display()))?;
        println!(
            "Wrote {} navigation report lines to {}.",
            dump_count,
            dump_path.display()
        );
    }
    Ok(())
}

fn replay_navigation(input_path: &Path) -> Result<(NavigationInput, HarnessContext)> {
    let mut input = read_navigation_input(input_path)?;
    let mut events = std::mem::take(&mut input.events);
    events.sort_by(|left, right| {
        left.timestamp()
            .partial_cmp(&right.timestamp())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut context = HarnessContext::with_default_reference_frame();
    for event in events {
        match event {
            SensorEvent::Imu(imu) => {
                let timestamp = imu.timestamp;
                context.handle_imu(timestamp, imu);
            }
            SensorEvent::LidarScan(scan) => {
                let timestamp = scan.timestamp;
                context.handle_lidar_scan(timestamp, scan);
            }
        }
    }
    Ok((input, context))
}

fn print_navigation_summary(input_path: &Path, input: &NavigationInput, context: &HarnessContext) {
    let input_name = input_path.to_string_lossy();
    let frame = canonical_reference_frame();
    let midpoint = navigation::to_local(&frame, &frame.global_center);
    let midpoint_east: f32 = midpoint.east.into();
    let midpoint_north: f32 = midpoint.north.into();
    let (position_error, heading_error) = context
        .positions
        .last()
        .map(|position| {
            let local = navigation::to_local(&frame, &position.coordinate);
            let east: f32 = local.east.into();
            let north: f32 = local.north.into();
            let heading: f32 = position.heading.into();
            let expected_heading = if input_name.contains("rotate90") {
                [135.0, 315.0]
            } else {
                [225.0, 225.0]
            };
            let error = expected_heading
                .into_iter()
                .map(|expected| angular_error_degrees(heading, expected))
                .fold(f32::INFINITY, f32::min);
            ((east - midpoint_east).hypot(north - midpoint_north), error)
        })
        .unwrap_or((f32::INFINITY, f32::INFINITY));
    let sw_hits = context
        .navigation_reports
        .iter()
        .flat_map(|report| report.corners.iter())
        .filter(|corner| corner.kind == navigation::CornerKind::Southwest)
        .count();
    let ne_hits = context
        .navigation_reports
        .iter()
        .flat_map(|report| report.corners.iter())
        .filter(|corner| corner.kind == navigation::CornerKind::Northeast)
        .count();

    println!(
        "Navigation summary: input={} input_format={} scans={} points={} accepted_points={} discarded_points={} revolutions={} wraps={} SW_hits={} NE_hits={} midpoint_position_error_m={:.4} heading_error_deg={:.2}",
        input_path.display(),
        input.format.name(),
        input.scan_count,
        input.point_count,
        context.navigation_context.accepted_points(),
        context.navigation_context.discarded_points(),
        context.navigation_context.accepted_revolutions(),
        context.navigation_context.revolution_wraps(),
        sw_hits,
        ne_hits,
        position_error,
        heading_error,
    );
    println!(
        "Harness outputs: {} positions, {} guidance updates, {} control updates, {} motor commands, {} telemetry messages.",
        context.positions.len(),
        context.guidance.len(),
        context.controls.len(),
        context.motor_commands.len(),
        context.telemetry.len(),
    );
}

fn dump_lines_for(
    input_path: &Path,
    input: &NavigationInput,
    context: &HarnessContext,
) -> Vec<DumpLine> {
    context
        .navigation_reports
        .iter()
        .map(|report| DumpLine {
            input: input_path.display().to_string(),
            input_format: input.format.name(),
            timestamp_ms: report.timestamp.into(),
            filled_bins: report.filled_bins,
            corners: report
                .corners
                .iter()
                .map(|corner| DumpCorner {
                    kind: match corner.kind {
                        navigation::CornerKind::Southwest => "southwest",
                        navigation::CornerKind::Northeast => "northeast",
                    },
                    vertex: [corner.vertex.0.into(), corner.vertex.1.into()],
                    range_m: corner.range_m.into(),
                    bearing_rad: corner.bearing_rad.into(),
                    interior_angle_rad: corner.interior_angle_rad.into(),
                    bisector_rad: corner.bisector_rad.into(),
                    score: corner.score.into(),
                })
                .collect(),
            pose: report.pose.as_ref().map(|pose| DumpPose {
                timestamp_ms: pose.timestamp.into(),
                latitude_deg: pose.coordinate.latitude.into(),
                longitude_deg: pose.coordinate.longitude.into(),
                heading_deg: pose.heading.into(),
            }),
        })
        .collect()
}

fn canonical_reference_frame() -> router::ReferenceFrame {
    router::ReferenceFrame {
        global_center: router::Coordinate {
            latitude: units::imperial::Degrees::new(39.0),
            longitude: units::imperial::Degrees::new(39.0),
        },
        global_diagonal: units::Length::new(78.0 * 111_319.5 * core::f32::consts::SQRT_2),
        local_diagonal: units::Length::new(78.0 * 111_319.5 / 1.0e7 * core::f32::consts::SQRT_2),
    }
}

fn angular_error_degrees(actual: f32, expected: f32) -> f32 {
    let error = (actual - expected).abs().rem_euclid(360.0);
    error.min(360.0 - error)
}

fn read_navigation_input(path: &Path) -> Result<NavigationInput> {
    let bytes = fs::read(path)
        .with_context(|| format!("could not read navigation input {}", path.display()))?;
    let is_json = path.extension().and_then(|extension| extension.to_str()) == Some("json")
        || bytes
            .iter()
            .copied()
            .find(|byte| !byte.is_ascii_whitespace())
            == Some(b'[');

    let (format, messages) = if is_json {
        let messages =
            serde_json::from_slice::<Vec<router::Message>>(&bytes).with_context(|| {
                format!("could not decode JSON navigation input {}", path.display())
            })?;
        (InputFormat::Json, messages)
    } else if let Some(messages) = decode_telemetry_messages(&bytes) {
        (InputFormat::Binary, messages)
    } else if let Some(scans) = decode_sensor_samples(&bytes, &|message| match message {
        router::Message::LidarScan(scan) => Some(scan),
        _ => None,
    }) {
        let point_count = scans.iter().map(|scan| scan.point_cloud.len()).sum();
        let scan_count = scans.len();
        let events = scans.into_iter().map(SensorEvent::LidarScan).collect();
        return Ok(NavigationInput {
            format: InputFormat::Binary,
            events,
            scan_count,
            point_count,
        });
    } else {
        anyhow::bail!(
            "navigation input {} is not valid binary or JSON telemetry",
            path.display()
        );
    };

    let mut scan_count = 0;
    let mut point_count = 0;
    let mut events = Vec::new();
    for message in messages {
        match message {
            router::Message::LidarScan(scan) => {
                scan_count += 1;
                point_count += scan.point_cloud.len();
                events.push(SensorEvent::LidarScan(scan));
            }
            router::Message::Imu(imu) => events.push(SensorEvent::Imu(imu)),
            _ => {}
        }
    }
    if scan_count == 0 {
        anyhow::bail!(
            "navigation input {} contains no LiDAR scans",
            path.display()
        );
    }
    Ok(NavigationInput {
        format,
        events,
        scan_count,
        point_count,
    })
}

/// Converts a recorded Postcard telemetry file to pretty-printed JSON.
///
/// The input is expected to use the length-prefixed `router::Message` format
/// written by the mission app. Messages are kept in recording order and are
/// serialized as tagged JSON values, so each telemetry variant retains its
/// fields instead of being reduced to LiDAR points. When no output path is
/// supplied, the input extension is replaced with `.json`.
pub fn convert_telemetry_to_json(input_path: &Path, output_path: Option<&Path>) -> Result<()> {
    let messages = read_telemetry_messages(input_path)?;
    let output_path = output_path.map(Path::to_path_buf).unwrap_or_else(|| {
        let mut path = input_path.to_path_buf();
        path.set_extension("json");
        path
    });

    let mut json = serde_json::to_vec_pretty(&messages)
        .context("could not encode telemetry messages as JSON")?;
    json.push(b'\n');
    fs::write(&output_path, json)
        .with_context(|| format!("could not write JSON output {}", output_path.display()))?;
    println!(
        "Converted {} telemetry messages from {} to {}.",
        messages.len(),
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

fn read_telemetry_messages(path: &Path) -> Result<Vec<router::Message>> {
    let bytes = fs::read(path)
        .with_context(|| format!("could not read telemetry input {}", path.display()))?;

    decode_telemetry_messages(&bytes).with_context(|| {
        format!(
            "telemetry input {} is not valid framed Postcard telemetry data",
            path.display()
        )
    })
}

fn decode_telemetry_messages(bytes: &[u8]) -> Option<Vec<router::Message>> {
    if let Some(messages) = decode_framed_messages(bytes, &Some) {
        return Some(messages);
    }

    decode_message_stream(bytes)
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

#[cfg(test)]
mod tests {
    use super::*;

    const BASELINE_JSON: &str =
        include_str!("../../../../telemetry_one_point_two_three_1791430559418.json");
    const ROTATED_JSON: &str =
        include_str!("../../../../telemetry_one_point_two_three_rotate90_1791430681872.json");

    fn fixture_counts(json: &str) -> (usize, usize, f32, f32) {
        let messages: Vec<router::Message> =
            serde_json::from_str(json).expect("canonical telemetry JSON should decode");
        let mut scan_count = 0;
        let mut point_count = 0;
        let mut first_timestamp = None;
        let mut last_timestamp = None;

        for message in messages {
            let router::Message::LidarScan(scan) = message else {
                panic!("canonical navigation fixture contains non-lidar telemetry");
            };
            scan_count += 1;
            point_count += scan.point_cloud.len();
            first_timestamp.get_or_insert(scan.timestamp);
            last_timestamp = Some(scan.timestamp);
        }

        let first_timestamp: f32 = first_timestamp
            .expect("canonical navigation fixture should not be empty")
            .into();
        let last_timestamp: f32 = last_timestamp
            .expect("canonical navigation fixture should not be empty")
            .into();
        (scan_count, point_count, first_timestamp, last_timestamp)
    }

    #[test]
    fn canonical_navigation_fixtures_keep_the_full_scan_sequences() {
        assert_eq!(
            fixture_counts(BASELINE_JSON),
            (119, 10_247, 312_886.0, 321_407.0)
        );
        assert_eq!(
            fixture_counts(ROTATED_JSON),
            (125, 10_966, 435_365.0, 445_418.0)
        );
    }

    #[test]
    fn phase_one_frame_helpers_preserve_the_flat_scaled_geometry() {
        let frame = router::ReferenceFrame {
            global_center: router::Coordinate {
                latitude: units::imperial::Degrees::new(4.5),
                longitude: units::imperial::Degrees::new(4.5),
            },
            global_diagonal: units::Length::new(9.0 * 111_319.5 * core::f32::consts::SQRT_2),
            local_diagonal: units::Length::new(9.0 * 111_319.5 / 1.0e7 * core::f32::consts::SQRT_2),
        };
        let coordinate = router::Coordinate {
            latitude: units::imperial::Degrees::new(1.25),
            longitude: units::imperial::Degrees::new(2.5),
        };
        let local = navigation::to_local(&frame, &coordinate);
        let round_trip = navigation::to_global(&frame, local);
        let latitude: f32 = round_trip.latitude.into();
        let longitude: f32 = round_trip.longitude.into();
        assert!((latitude - 1.25).abs() < 0.0001);
        assert!((longitude - 2.5).abs() < 0.0001);

        let origin = navigation::to_local(&frame, &frame.global_center);
        let origin_east: f32 = origin.east.into();
        let origin_north: f32 = origin.north.into();
        assert!(origin_east.abs() < f32::EPSILON);
        assert!(origin_north.abs() < f32::EPSILON);

        let northeast = navigation::northeast_local(&frame);
        let east: f32 = northeast.east.into();
        let north: f32 = northeast.north.into();
        let expected_diagonal: f32 = frame.local_diagonal.into();
        assert!((east.hypot(north) - expected_diagonal / 2.0).abs() < 0.001);
        let expected_component = expected_diagonal * 0.5 * core::f32::consts::FRAC_1_SQRT_2;
        assert!((east - expected_component).abs() < 0.001);
        assert!((north - expected_component).abs() < 0.001);

        let larger_diagonal_frame = router::ReferenceFrame {
            local_diagonal: frame.local_diagonal * units::Scalar::new(100.0),
            ..frame.clone()
        };
        let larger_diagonal_northeast = navigation::northeast_local(&larger_diagonal_frame);
        let larger_diagonal_east: f32 = larger_diagonal_northeast.east.into();
        let larger_diagonal_north: f32 = larger_diagonal_northeast.north.into();
        assert!((larger_diagonal_east - expected_component * 100.0).abs() < 0.02);
        assert!((larger_diagonal_north - expected_component * 100.0).abs() < 0.02);

        let axis_cases = [
            (0.0, 1.0, 0.0),
            (core::f32::consts::FRAC_PI_2, 0.0, -1.0),
            (core::f32::consts::PI, -1.0, 0.0),
            (3.0 * core::f32::consts::PI / 2.0, 0.0, 1.0),
        ];
        for (angle, expected_x, expected_y) in axis_cases {
            let (x, y) =
                navigation::lidar_to_robot(units::PlaneAngle::new(angle), units::Length::new(1.0));
            let x: f32 = x.into();
            let y: f32 = y.into();
            assert!((x - expected_x).abs() < 0.0001);
            assert!((y - expected_y).abs() < 0.0001);
        }

        let bearing: f32 = navigation::baseline_bearing(&frame).into();
        assert!((bearing - core::f32::consts::FRAC_PI_4).abs() < 0.0001);
        let wrapped_heading: f32 =
            navigation::heading_to_degrees(units::PlaneAngle::new(-0.25)).into();
        assert!((wrapped_heading - (360.0 - 0.25 * 180.0 / core::f32::consts::PI)).abs() < 0.001);

        let wrapped_zero: f32 =
            navigation::wrap_2pi(units::PlaneAngle::new(-2.0 * core::f32::consts::PI)).into();
        let wrapped_pi: f32 =
            navigation::wrap_pi(units::PlaneAngle::new(3.0 * core::f32::consts::PI)).into();
        assert!(wrapped_zero.abs() < 0.001);
        assert!((wrapped_pi - core::f32::consts::PI).abs() < 0.001);
    }

    fn scans_from_json(json: &str) -> Vec<router::Scan> {
        serde_json::from_str::<Vec<router::Message>>(json)
            .expect("canonical telemetry JSON should decode")
            .into_iter()
            .map(|message| match message {
                router::Message::LidarScan(scan) => scan,
                _ => panic!("canonical navigation fixture contains non-lidar telemetry"),
            })
            .collect()
    }

    fn scans_from_data(data: &[u8]) -> Vec<router::Scan> {
        decode_sensor_samples(data, &|message| match message {
            router::Message::LidarScan(scan) => Some(scan),
            _ => None,
        })
        .expect("canonical binary telemetry should decode")
    }

    fn replay_revolutions(
        scans: &[router::Scan],
    ) -> (navigation::Context, Vec<Vec<(f32, f32, f32, usize)>>) {
        let mut context = navigation::Context::default();
        let frame = router::ReferenceFrame::default();
        let mut revolutions = Vec::new();
        for scan in scans {
            context.update(scan.timestamp, None, Some(scan.clone()), None, &frame);
            while let Some(revolution) = context.take_completed_revolution() {
                revolutions.push(
                    revolution
                        .points()
                        .map(|(x, y, range, bin)| (x.into(), y.into(), range.into(), bin))
                        .collect(),
                );
            }
        }
        (context, revolutions)
    }

    fn assert_phase_two_fixture(
        name: &str,
        json: &str,
        data: &[u8],
        expected_scans: usize,
        expected_points: usize,
    ) {
        let json_scans = scans_from_json(json);
        let binary_scans = scans_from_data(data);
        assert_eq!(json_scans.len(), expected_scans, "{name} JSON scan count");
        assert_eq!(
            binary_scans.len(),
            expected_scans,
            "{name} binary scan count"
        );
        assert_eq!(
            json_scans
                .iter()
                .map(|scan| scan.point_cloud.len())
                .sum::<usize>(),
            expected_points,
            "{name} JSON point count"
        );
        assert_eq!(
            binary_scans
                .iter()
                .map(|scan| scan.point_cloud.len())
                .sum::<usize>(),
            expected_points,
            "{name} binary point count"
        );

        let (json_context, json_revolutions) = replay_revolutions(&json_scans);
        let (binary_context, binary_revolutions) = replay_revolutions(&binary_scans);
        assert_eq!(json_context.accepted_points(), expected_points as u32);
        assert_eq!(json_context.discarded_points(), 0);
        assert!(
            json_context.revolution_wraps() > 0,
            "{name} should wrap angles"
        );
        assert!(
            json_context.accepted_revolutions() > 0,
            "{name} should contain a complete revolution"
        );
        assert!(
            json_revolutions
                .iter()
                .all(|revolution| revolution.len() >= navigation::MIN_BINS as usize),
            "{name} completed revolutions must meet the bin threshold"
        );
        assert_eq!(
            json_revolutions, binary_revolutions,
            "{name} JSON/binary bins"
        );
        assert_eq!(
            json_context.revolution_wraps(),
            binary_context.revolution_wraps(),
            "{name} JSON/binary boundaries"
        );
        println!(
            "{name}: accepted={} discarded={} wraps={} complete={} discarded_revolutions={} bins={:?}",
            json_context.accepted_points(),
            json_context.discarded_points(),
            json_context.revolution_wraps(),
            json_context.accepted_revolutions(),
            json_context.discarded_revolutions(),
            json_revolutions.iter().map(Vec::len).collect::<Vec<_>>()
        );
    }

    #[test]
    fn phase_two_accumulates_baseline_json_and_binary_revolutions() {
        const BASELINE_DATA: &[u8] =
            include_bytes!("../../../../telemetry_one_point_two_three_1791430559418.data");
        assert_phase_two_fixture("baseline", BASELINE_JSON, BASELINE_DATA, 119, 10_247);
    }

    #[test]
    fn phase_two_accumulates_rotated_json_and_binary_revolutions() {
        const ROTATED_DATA: &[u8] =
            include_bytes!("../../../../telemetry_one_point_two_three_rotate90_1791430681872.data");
        assert_phase_two_fixture("rotated", ROTATED_JSON, ROTATED_DATA, 125, 10_966);
    }

    #[test]
    fn phase_three_detects_a_synthetic_convex_corner() {
        let vertex = (1.2_f32, 1.0_f32);
        let bisector = vertex.1.atan2(vertex.0);
        let mut points = Vec::new();
        for side in [-1.0_f32, 1.0_f32] {
            for step in 0..32 {
                let distance = 0.04 + step as f32 * 0.01;
                let wall_angle = bisector + side * core::f32::consts::FRAC_PI_3;
                let x = vertex.0 + distance * wall_angle.cos();
                let y = vertex.1 + distance * wall_angle.sin();
                let raw_angle = (-y.atan2(x)).rem_euclid(2.0 * core::f32::consts::PI);
                let range = (x * x + y * y).sqrt();
                points.push((units::PlaneAngle::new(raw_angle), units::Length::new(range)));
            }
        }
        points.push((
            units::PlaneAngle::new(-bisector),
            units::Length::new(vertex.0.hypot(vertex.1)),
        ));
        let revolution = navigation::Revolution::from_points(units::Time::new(1.0), &points);

        let corners = navigation::detect_corners(&revolution);
        let observation = corners
            .iter()
            .find(|corner| corner.kind == navigation::CornerKind::Southwest)
            .expect("synthetic convex corner should be detected");
        let detected_x: f32 = observation.vertex.0.into();
        let detected_y: f32 = observation.vertex.1.into();
        let interior: f32 = observation.interior_angle_rad.into();
        assert!(
            (detected_x - vertex.0).abs() < 0.02,
            "detected=({}, {}) expected=({}, {})",
            detected_x,
            detected_y,
            vertex.0,
            vertex.1
        );
        assert!((detected_y - vertex.1).abs() < 0.02);
        assert!((interior.to_degrees() - 120.0).abs() < 3.0);
    }

    #[test]
    fn phase_three_detects_a_corner_straddling_the_zero_degree_bin() {
        let vertex = (1.5_f32, 0.0_f32);
        let bisector = 0.0_f32;
        let mut points = Vec::new();
        for side in [-1.0_f32, 1.0_f32] {
            for step in 0..32 {
                let distance = 0.04 + step as f32 * 0.01;
                let wall_angle = bisector + side * core::f32::consts::FRAC_PI_3;
                let x = vertex.0 + distance * wall_angle.cos();
                let y = vertex.1 + distance * wall_angle.sin();
                let raw_angle = (-y.atan2(x)).rem_euclid(2.0 * core::f32::consts::PI);
                points.push((
                    units::PlaneAngle::new(raw_angle),
                    units::Length::new(x.hypot(y)),
                ));
            }
        }
        points.push((units::PlaneAngle::new(0.0), units::Length::new(vertex.0)));
        let revolution = navigation::Revolution::from_points(units::Time::new(1.0), &points);

        let corners = navigation::detect_corners(&revolution);
        let observation = corners
            .iter()
            .find(|corner| corner.kind == navigation::CornerKind::Southwest)
            .expect("a convex corner on the 0-degree seam should be detected");
        let detected_x: f32 = observation.vertex.0.into();
        let detected_y: f32 = observation.vertex.1.into();
        assert!((detected_x - vertex.0).abs() < 0.02, "x={detected_x}");
        assert!((detected_y - vertex.1).abs() < 0.02, "y={detected_y}");
    }

    #[test]
    fn phase_three_detects_a_synthetic_concave_corner() {
        let vertex = (1.2_f32, 1.0_f32);
        let bisector = (-vertex.1).atan2(-vertex.0);
        let mut points = Vec::new();
        for side in [-1.0_f32, 1.0_f32] {
            for step in 0..32 {
                let distance = 0.04 + step as f32 * 0.01;
                let wall_angle = bisector + side * core::f32::consts::FRAC_PI_3;
                let x = vertex.0 + distance * wall_angle.cos();
                let y = vertex.1 + distance * wall_angle.sin();
                points.push((
                    units::PlaneAngle::new((-y.atan2(x)).rem_euclid(2.0 * core::f32::consts::PI)),
                    units::Length::new(x.hypot(y)),
                ));
            }
        }
        points.push((
            units::PlaneAngle::new(
                (-vertex.1.atan2(vertex.0)).rem_euclid(2.0 * core::f32::consts::PI),
            ),
            units::Length::new(vertex.0.hypot(vertex.1)),
        ));

        let revolution = navigation::Revolution::from_points(units::Time::new(1.0), &points);
        let corners = navigation::detect_corners(&revolution);
        let observation = corners
            .iter()
            .find(|corner| corner.kind == navigation::CornerKind::Northeast)
            .expect("synthetic concave corner should be detected");
        let detected_x: f32 = observation.vertex.0.into();
        let detected_y: f32 = observation.vertex.1.into();
        let interior: f32 = observation.interior_angle_rad.into();
        assert!((detected_x - vertex.0).abs() < 0.02, "x={detected_x}");
        assert!((detected_y - vertex.1).abs() < 0.02, "y={detected_y}");
        assert!((interior.to_degrees() - 120.0).abs() < 3.0);
    }

    #[test]
    fn phase_three_detector_reports_fixture_observations() {
        for (name, json) in [("baseline", BASELINE_JSON), ("rotated", ROTATED_JSON)] {
            let scans = scans_from_json(json);
            let mut context = navigation::Context::default();
            let frame = router::ReferenceFrame::default();
            let mut observations = 0;
            for scan in scans {
                context.update(scan.timestamp, None, Some(scan), None, &frame);
                while let Some(revolution) = context.take_completed_revolution() {
                    println!("{name}: revolution bins={}", revolution.points().count());
                    let corners = navigation::detect_corners(&revolution);
                    observations += corners.len();
                    for corner in corners {
                        let vertex: (f32, f32) = (corner.vertex.0.into(), corner.vertex.1.into());
                        let score: f32 = corner.score.into();
                        let interior: f32 = corner.interior_angle_rad.into();
                        println!(
                            "{name}: {:?} vertex=({:.3}, {:.3}) interior={:.1} score={:.2}",
                            corner.kind,
                            vertex.0,
                            vertex.1,
                            interior.to_degrees(),
                            score,
                        );
                    }
                }
            }
            assert!(
                observations > 0,
                "{name} should produce corner observations"
            );
        }
    }

    fn phase_four_test_frame() -> router::ReferenceFrame {
        router::ReferenceFrame {
            global_center: router::Coordinate {
                latitude: units::imperial::Degrees::new(0.5),
                longitude: units::imperial::Degrees::new(0.5),
            },
            global_diagonal: units::Length::new(111_319.5 * core::f32::consts::SQRT_2),
            local_diagonal: units::Length::new(core::f32::consts::SQRT_2),
        }
    }

    fn phase_four_observation(
        kind: navigation::CornerKind,
        frame: &router::ReferenceFrame,
        position: (f32, f32),
        heading: f32,
        score: f32,
    ) -> navigation::CornerObservation {
        let northeast = navigation::northeast_local(frame);
        let corner: (f32, f32) = match kind {
            navigation::CornerKind::Southwest => (
                (northeast.east * units::Scalar::new(-1.0)).into(),
                (northeast.north * units::Scalar::new(-1.0)).into(),
            ),
            navigation::CornerKind::Northeast => (northeast.east.into(), northeast.north.into()),
        };
        let east = corner.0 - position.0;
        let north = corner.1 - position.1;
        let x = east * heading.sin() + north * heading.cos();
        let y = -east * heading.cos() + north * heading.sin();
        let range = x.hypot(y);
        let bearing = y.atan2(x);
        let baseline_bearing: f32 = navigation::baseline_bearing(frame).into();
        let global_bisector = navigation::wrap_2pi(units::PlaneAngle::new(
            baseline_bearing + core::f32::consts::PI,
        ));
        let global_bisector: f32 = global_bisector.into();
        let bisector = navigation::wrap_2pi(units::PlaneAngle::new(heading - global_bisector));
        let interior = match kind {
            navigation::CornerKind::Southwest => navigation::SW_INTERIOR_ANGLE,
            navigation::CornerKind::Northeast => navigation::NE_INTERIOR_ANGLE,
        };
        navigation::CornerObservation {
            kind,
            vertex: (units::Length::new(x), units::Length::new(y)),
            range_m: units::Length::new(range),
            bearing_rad: units::PlaneAngle::new(bearing),
            interior_angle_rad: interior,
            bisector_rad: bisector,
            score: units::Scalar::new(score),
        }
    }

    #[test]
    fn phase_four_recovers_one_corner_pose_for_multiple_headings() {
        let frame = phase_four_test_frame();
        let expected_position = (0.35_f32, 0.42_f32);
        for expected_heading in [0.2_f32, 1.1, 3.8, 5.9] {
            let observation = phase_four_observation(
                navigation::CornerKind::Southwest,
                &frame,
                expected_position,
                expected_heading,
                0.9,
            );
            let pose = navigation::fix_from_corners(
                &frame,
                &[observation],
                Some(units::PlaneAngle::new(expected_heading)),
            )
            .expect("one-corner observation with a prior heading should produce a fix");
            let east: f32 = pose.position.east.into();
            let north: f32 = pose.position.north.into();
            let heading: f32 = pose.heading.into();
            assert!((east - expected_position.0).abs() < 0.0001);
            assert!((north - expected_position.1).abs() < 0.0001);
            let heading_error: f32 =
                navigation::wrap_pi(units::PlaneAngle::new(heading - expected_heading)).into();
            assert!(heading_error.abs() < 0.0001);
            assert!(
                navigation::fix_from_corners(&frame, &[observation], None).is_none(),
                "one corner without a prior heading cannot fix heading"
            );
        }
    }

    #[test]
    fn phase_four_uses_two_corner_consistency_and_baseline_heading() {
        let frame = phase_four_test_frame();
        let expected_position = (0.35_f32, 0.42_f32);
        let expected_heading = 1.1_f32;
        let southwest = phase_four_observation(
            navigation::CornerKind::Southwest,
            &frame,
            expected_position,
            expected_heading,
            0.8,
        );
        let northeast = phase_four_observation(
            navigation::CornerKind::Northeast,
            &frame,
            expected_position,
            expected_heading,
            0.9,
        );
        let pose = navigation::fix_from_corners(&frame, &[southwest, northeast], None)
            .expect("consistent corner observations should produce a fix");
        let east: f32 = pose.position.east.into();
        let north: f32 = pose.position.north.into();
        let heading: f32 = pose.heading.into();
        assert!((east - expected_position.0).abs() < 0.0001);
        assert!((north - expected_position.1).abs() < 0.0001);
        let heading_error: f32 =
            navigation::wrap_pi(units::PlaneAngle::new(heading - expected_heading)).into();
        assert!(heading_error.abs() < 0.0001);

        let mut inconsistent = northeast;
        let range: f32 = inconsistent.range_m.into();
        inconsistent.range_m = units::Length::new(range + 0.2);
        assert!(navigation::fix_from_corners(&frame, &[southwest, inconsistent], None).is_none());
    }

    #[test]
    fn phase_four_fixture_fixes_stay_near_the_arena_midpoint() {
        let frame = router::ReferenceFrame {
            global_center: router::Coordinate {
                latitude: units::imperial::Degrees::new(39.0),
                longitude: units::imperial::Degrees::new(39.0),
            },
            global_diagonal: units::Length::new(78.0 * 111_319.5 * core::f32::consts::SQRT_2),
            local_diagonal: units::Length::new(
                78.0 * 111_319.5 / 1.0e7 * core::f32::consts::SQRT_2,
            ),
        };
        let midpoint = navigation::to_local(&frame, &frame.global_center);
        let midpoint_east: f32 = midpoint.east.into();
        let midpoint_north: f32 = midpoint.north.into();
        let expected_heading = 225.0_f32;
        let mut heading_samples = Vec::new();

        for (name, json) in [("baseline", BASELINE_JSON), ("rotated", ROTATED_JSON)] {
            let scans = scans_from_json(json);
            let mut context = navigation::Context::default();
            let mut fixes = Vec::new();
            for scan in scans {
                context.update(scan.timestamp, None, Some(scan), None, &frame);
                while let Some(revolution) = context.take_completed_revolution() {
                    let corners = navigation::detect_corners(&revolution);
                    if let Some(pose) = navigation::fix_from_corners(&frame, &corners, None) {
                        fixes.push(pose);
                    }
                }
            }
            assert!(!fixes.is_empty(), "{name} should produce corner fixes");
            for pose in fixes {
                let east: f32 = pose.position.east.into();
                let north: f32 = pose.position.north.into();
                assert!((east - midpoint_east).hypot(north - midpoint_north) < 0.05);
                let heading: f32 = pose.heading.into();
                heading_samples.push((name, heading.to_degrees()));
            }
        }

        let baseline_heading = heading_samples
            .iter()
            .find(|(name, _)| *name == "baseline")
            .map(|(_, heading)| *heading)
            .expect("baseline should produce a heading sample");
        let rotated_heading = heading_samples
            .iter()
            .find(|(name, _)| *name == "rotated")
            .map(|(_, heading)| *heading)
            .expect("rotated should produce a heading sample");
        let baseline_error = (baseline_heading - expected_heading).abs();
        assert!(baseline_error.min(360.0 - baseline_error) < 5.0);
        let difference = (rotated_heading - baseline_heading).abs();
        assert!(difference.min(360.0 - difference) > 85.0);
        assert!(difference.min(360.0 - difference) < 95.0);
    }

    fn integrated_positions(
        scans: &[router::Scan],
        frame: &router::ReferenceFrame,
    ) -> Vec<router::Position> {
        let mut context = navigation::Context::default();
        scans
            .iter()
            .filter_map(|scan| {
                context.update(scan.timestamp, None, Some(scan.clone()), None, frame)
            })
            .collect()
    }

    #[test]
    fn phase_six_integrated_replay_matches_binary_and_json_fixtures() {
        let frame = router::ReferenceFrame {
            global_center: router::Coordinate {
                latitude: units::imperial::Degrees::new(39.0),
                longitude: units::imperial::Degrees::new(39.0),
            },
            global_diagonal: units::Length::new(78.0 * 111_319.5 * core::f32::consts::SQRT_2),
            local_diagonal: units::Length::new(
                78.0 * 111_319.5 / 1.0e7 * core::f32::consts::SQRT_2,
            ),
        };
        let mut final_headings = Vec::new();
        for (name, json, data) in [
            (
                "baseline",
                BASELINE_JSON,
                include_bytes!("../../../../telemetry_one_point_two_three_1791430559418.data")
                    as &[u8],
            ),
            (
                "rotated",
                ROTATED_JSON,
                include_bytes!(
                    "../../../../telemetry_one_point_two_three_rotate90_1791430681872.data"
                ) as &[u8],
            ),
        ] {
            let json_positions = integrated_positions(&scans_from_json(json), &frame);
            let binary_positions = integrated_positions(&scans_from_data(data), &frame);
            assert!(
                !json_positions.is_empty(),
                "{name} should emit integrated fixes"
            );
            assert!(
                json_positions == binary_positions,
                "{name} binary/JSON poses differ"
            );

            let final_position = json_positions
                .last()
                .expect("fixture should have a final pose");
            let local = navigation::to_local(&frame, &final_position.coordinate);
            let midpoint = navigation::to_local(&frame, &frame.global_center);
            let east: f32 = local.east.into();
            let north: f32 = local.north.into();
            let midpoint_east: f32 = midpoint.east.into();
            let midpoint_north: f32 = midpoint.north.into();
            assert!((east - midpoint_east).hypot(north - midpoint_north) <= 0.05);
            let heading: f32 = final_position.heading.into();
            final_headings.push(heading);
        }

        let difference = (final_headings[1] - final_headings[0]).abs();
        assert!((difference.min(360.0 - difference) - 90.0).abs() <= 5.0);
    }

    fn phase_five_imu(accel: (f32, f32, f32), gyro: (f32, f32, f32)) -> router::Imu {
        router::Imu {
            timestamp: units::Time::new(0.0),
            accel: router::Accel {
                x: units::Acceleration::new(accel.0),
                y: units::Acceleration::new(accel.1),
                z: units::Acceleration::new(accel.2),
            },
            gyro: router::Gyro {
                x: units::AngularVelocity::new(gyro.0),
                y: units::AngularVelocity::new(gyro.1),
                z: units::AngularVelocity::new(gyro.2),
            },
        }
    }

    fn phase_five_seed_position(frame: &router::ReferenceFrame) -> router::Position {
        router::Position {
            timestamp: units::Time::new(0.0),
            coordinate: frame.global_center.clone(),
            heading: units::imperial::Degrees::new(0.0),
        }
    }

    #[test]
    fn phase_five_imu_without_a_prior_position_returns_none() {
        let mut context = navigation::Context::default();
        let frame = phase_four_test_frame();
        let imu = phase_five_imu((0.0, 0.0, 0.0), (0.0, 0.0, 1.0));
        assert!(
            context
                .update(units::Time::new(100.0), None, None, Some(imu), &frame,)
                .is_none()
        );
    }

    #[test]
    fn phase_five_constant_yaw_rate_uses_the_compass_sign() {
        let mut context = navigation::Context::default();
        let frame = phase_four_test_frame();
        let seed = phase_five_seed_position(&frame);
        let _ = context.update(seed.timestamp, Some(&seed), None, None, &frame);
        let _ = context.update(
            units::Time::new(0.0),
            None,
            None,
            Some(phase_five_imu((0.0, 0.0, 0.0), (0.0, 0.0, 1.0))),
            &frame,
        );

        let mut last_position = None;
        for timestamp in (100..=1_000).step_by(100) {
            let imu = phase_five_imu((0.0, 0.0, 0.0), (0.0, 0.0, 1.0));
            if let Some(position) = context.update(
                units::Time::new(timestamp as f32),
                None,
                None,
                Some(imu),
                &frame,
            ) {
                last_position = Some(position);
            }
        }

        let position = last_position.expect("dead reckoning should emit a throttled position");
        let heading: f32 = position.heading.into();
        let expected_heading = (360.0 - 1.0_f32.to_degrees()).rem_euclid(360.0);
        let error = (heading - expected_heading).abs();
        assert!(error.min(360.0 - error) < 0.1, "heading={heading}");
    }

    #[test]
    fn phase_five_zero_imu_keeps_the_seed_pose() {
        let mut context = navigation::Context::default();
        let frame = phase_four_test_frame();
        let seed = phase_five_seed_position(&frame);
        let _ = context.update(seed.timestamp, Some(&seed), None, None, &frame);

        let _ = context.update(
            units::Time::new(0.0),
            None,
            None,
            Some(phase_five_imu((0.0, 0.0, 0.0), (0.0, 0.0, 0.0))),
            &frame,
        );
        let position = context
            .update(
                units::Time::new(100.0),
                None,
                None,
                Some(phase_five_imu((0.0, 0.0, 0.0), (0.0, 0.0, 0.0))),
                &frame,
            )
            .expect("zero IMU should still emit a position");
        let east: f32 = navigation::to_local(&frame, &position.coordinate)
            .east
            .into();
        let north: f32 = navigation::to_local(&frame, &position.coordinate)
            .north
            .into();
        let heading: f32 = position.heading.into();
        assert!(east.abs() < 0.0001);
        assert!(north.abs() < 0.0001);
        assert!(heading.abs() < 0.0001);
    }

    #[test]
    fn phase_five_stationary_bias_estimation_and_zupt_bound_drift() {
        let mut context = navigation::Context::default();
        let frame = phase_four_test_frame();
        let seed = phase_five_seed_position(&frame);
        let _ = context.update(seed.timestamp, Some(&seed), None, None, &frame);
        let mut outputs = Vec::new();
        for timestamp in (0..=5_000).step_by(50) {
            let imu = phase_five_imu((0.2, -0.1, 9.81), (0.0, 0.0, 0.01));
            if let Some(position) = context.update(
                units::Time::new(timestamp as f32),
                None,
                None,
                Some(imu),
                &frame,
            ) {
                outputs.push(position);
            }
        }

        let final_position = outputs.last().expect("stationary IMU should emit output");
        let local = navigation::to_local(&frame, &final_position.coordinate);
        let east: f32 = local.east.into();
        let north: f32 = local.north.into();
        assert!(east.abs() < 0.0001);
        assert!(north.abs() < 0.0001);

        let last_heading: f32 = outputs[outputs.len() - 1].heading.into();
        let previous_heading: f32 = outputs[outputs.len() - 2].heading.into();
        let heading_step = navigation::wrap_pi(units::PlaneAngle::new(
            (last_heading - previous_heading).to_radians(),
        ));
        let heading_step: f32 = heading_step.into();
        assert!(heading_step.abs() < 0.005);
    }

    #[test]
    fn phase_five_timestamp_gaps_are_clamped_to_one_hundred_milliseconds() {
        let mut context = navigation::Context::default();
        let frame = phase_four_test_frame();
        let seed = phase_five_seed_position(&frame);
        let _ = context.update(seed.timestamp, Some(&seed), None, None, &frame);
        let imu = phase_five_imu((0.0, 0.0, 0.0), (0.0, 0.0, 1.0));
        let _ = context.update(units::Time::new(0.0), None, None, Some(imu.clone()), &frame);
        let first = context
            .update(
                units::Time::new(1_000.0),
                None,
                None,
                Some(imu.clone()),
                &frame,
            )
            .expect("clamped gap should emit a position");
        let second = context
            .update(units::Time::new(1_100.0), None, None, Some(imu), &frame)
            .expect("next output period should emit a position");
        let first_heading: f32 = first.heading.into();
        let second_heading: f32 = second.heading.into();
        let change = navigation::wrap_pi(units::PlaneAngle::new(
            (second_heading - first_heading).to_radians(),
        ));
        let change: f32 = change.into();
        assert!((change + 0.1).abs() < 0.001, "change={change}");
    }

    #[test]
    fn context_update_without_a_scan_does_not_create_a_revolution() {
        let mut context = navigation::Context::default();
        assert!(
            context
                .update(
                    units::Time::new(0.0),
                    None,
                    None,
                    None,
                    &router::ReferenceFrame::default(),
                )
                .is_none()
        );
        assert!(context.current_revolution().is_none());
        assert_eq!(context.revolution_wraps(), 0);
    }
}
