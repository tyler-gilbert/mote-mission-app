//! Guidance telemetry as a map of the route with the current guidance vector.
//!
//! Plot coordinates are east/north offsets in global metres from the reference
//! frame's center, using the same flat-earth convention (one degree is
//! [`METRES_PER_DEGREE`] on both axes) as the Mote's guidance. The guidance
//! vector therefore ends at the waypoint the Mote is steering to.
//!
//! The view starts on the reference frame and can be zoomed and panned. Zooming scales
//! both axes together so east and north distances stay undistorted.

use bee_ui::{
    UiRoot,
    prelude::{
        AxisZoomMode, Button, FollowMode, HStack, ScatterArrow, ScatterDataset, ScatterLine,
        ScatterPlot, ScatterPlotProps, ScatterPoint, ScatterSeries, ScatterShape, Variant,
        ViewportState,
    },
};
use eframe::egui;
use mote_api::{messages::router, work::navigation::METRES_PER_DEGREE};

use crate::transport::LatestTelemetry;

const GUIDANCE_PLOT_ID: &str = "mote.telemetry.guidance";
/// The view shows the reference square plus a 20% margin, matching the LiDAR reference view.
const GUIDANCE_VIEW_SCALE: f64 = 1.2;

const REFERENCE_FRAME_COLOR: egui::Color32 = egui::Color32::from_rgb(40, 120, 240);
const ROUTE_COLOR: egui::Color32 = egui::Color32::MAGENTA;
const POSITION_COLOR: egui::Color32 = egui::Color32::from_rgb(40, 40, 40);
const GUIDANCE_COLOR: egui::Color32 = egui::Color32::from_rgb(253, 126, 20);

const REFERENCE_FRAME_LINE_WIDTH: f32 = 1.5;
const ROUTE_LINE_WIDTH: f32 = 2.0;
const ROUTE_POINT_SIZE: f32 = 4.0;
const POSITION_POINT_SIZE: f32 = 6.0;
const GUIDANCE_LINE_WIDTH: f32 = 3.0;
const GUIDANCE_HEAD_LENGTH: f32 = 14.0;
/// The heading arrow keeps this on-screen length at every zoom; it shows direction only.
const HEADING_ARROW_LENGTH: f32 = 40.0;
const HEADING_LINE_WIDTH: f32 = 2.0;
const HEADING_HEAD_LENGTH: f32 = 10.0;

pub(super) fn render(
    ui: &mut UiRoot<'_>,
    context: &mut Context,
    telemetry: &LatestTelemetry,
    reference_frame: Option<&router::ReferenceFrame>,
    route: Option<&router::Route>,
) {
    let Some((reference_frame, half_diagonal)) =
        reference_frame.and_then(|frame| half_diagonal(frame).map(|half| (frame, half)))
    else {
        ui.raw(|raw_ui| {
            raw_ui.label("Load mission settings with a reference frame to plot guidance.")
        });
        return;
    };

    context.apply_reference_frame(reference_frame, half_diagonal);
    let dataset = dataset(reference_frame, half_diagonal, route, telemetry);

    // Shown above the plot so the plot can take all of the remaining height.
    ui.hstack(HStack::new(), |ui| {
        let reset = ui.button(Button::new("Reset view").variant(Variant::Secondary));
        if reset
            .on_hover_text("Show the whole reference frame")
            .clicked()
        {
            context.viewport = reference_viewport(half_diagonal);
        }
        if let Some(waiting_for) = waiting_for(telemetry) {
            ui.raw(|raw_ui| raw_ui.label(format!("Waiting for {waiting_for} telemetry…")));
        }
    });

    ui.raw(|raw_ui| {
        let props = ScatterPlotProps::new(GUIDANCE_PLOT_ID)
            .axis_labels("East (m)", "North (m)")
            .height(raw_ui.available_height())
            .fill_width(true)
            .legend(true)
            .lock_view(true)
            .uniform_zoom(true)
            .drag_to_pan(true);
        ScatterPlot::show_with_viewport(raw_ui, &props, &dataset, &mut context.viewport);
    });
}

#[derive(Default)]
pub(super) struct Context {
    viewport: ViewportState,
    reference_frame_hash: Option<u32>,
}

impl Context {
    /// Resets the view to the reference frame whenever a different frame is loaded, and
    /// otherwise keeps the user's zoom and pan.
    fn apply_reference_frame(&mut self, reference_frame: &router::ReferenceFrame, half: f64) {
        let hash = reference_frame.hash();
        if self.reference_frame_hash != Some(hash) {
            self.reference_frame_hash = Some(hash);
            self.viewport = reference_viewport(half);
        }
    }
}

fn waiting_for(telemetry: &LatestTelemetry) -> Option<&'static str> {
    if telemetry.guidance.is_none() {
        Some("Guidance")
    } else if telemetry.position.is_none() {
        Some("Position")
    } else {
        None
    }
}

fn dataset(
    reference_frame: &router::ReferenceFrame,
    half_diagonal: f64,
    route: Option<&router::Route>,
    telemetry: &LatestTelemetry,
) -> ScatterDataset {
    let center = &reference_frame.global_center;
    let position = telemetry
        .position
        .as_ref()
        .and_then(|position| plot_position(center, position));

    let mut series = vec![
        ScatterSeries::new("mote.telemetry.guidance.reference_frame", "Reference frame")
            .with_points(reference_square(half_diagonal))
            .line(ScatterLine::new(
                REFERENCE_FRAME_LINE_WIDTH,
                REFERENCE_FRAME_COLOR,
            ))
            .show_markers(false),
    ];

    if let Some(route) = route.filter(|route| !route.is_empty()) {
        let waypoints = route
            .iter()
            .filter_map(|coordinate| to_plot(center, coordinate))
            .map(|(east, north)| {
                ScatterPoint::new(east, north)
                    .size(ROUTE_POINT_SIZE)
                    .color(ROUTE_COLOR)
            })
            .collect();
        series.push(
            ScatterSeries::new("mote.telemetry.guidance.route", "Route")
                .with_points(waypoints)
                .line(ScatterLine::new(ROUTE_LINE_WIDTH, ROUTE_COLOR)),
        );
    }

    if let Some((origin, heading)) = position {
        series.push(
            ScatterSeries::new("mote.telemetry.guidance.position", "Position").with_points(vec![
                ScatterPoint::new(origin.0, origin.1)
                    .shape(ScatterShape::Diamond)
                    .size(POSITION_POINT_SIZE)
                    .color(POSITION_COLOR),
            ]),
        );
        series.push(
            ScatterSeries::new("mote.telemetry.guidance.heading", "Heading").with_arrows(vec![
                ScatterArrow::fixed_length(
                    origin,
                    heading_direction(heading),
                    HEADING_ARROW_LENGTH,
                )
                .width(HEADING_LINE_WIDTH)
                .head_length(HEADING_HEAD_LENGTH)
                .color(POSITION_COLOR),
            ]),
        );
    }

    if let (Some((origin, heading)), Some(guidance)) = (position, telemetry.guidance.as_ref())
        && let Some(tip) = guidance_tip(origin, heading, guidance)
    {
        series.push(
            ScatterSeries::new("mote.telemetry.guidance.vector", "Guidance").with_arrows(vec![
                ScatterArrow::new(origin, tip)
                    .width(GUIDANCE_LINE_WIDTH)
                    .head_length(GUIDANCE_HEAD_LENGTH)
                    .color(GUIDANCE_COLOR),
            ]),
        );
    }

    ScatterDataset::new(series)
}

/// Half of the reference frame's global diagonal, in metres, when it describes a usable area.
fn half_diagonal(reference_frame: &router::ReferenceFrame) -> Option<f64> {
    let diagonal: f32 = reference_frame.global_diagonal.into();
    let half_diagonal = f64::from(diagonal) * 0.5;
    (half_diagonal.is_finite() && half_diagonal > 0.0).then_some(half_diagonal)
}

/// Square view centered on the frame, sized from the frame's global diagonal.
/// The equal aspect ratio lets the plot fill its area by widening the shorter axis while
/// keeping the whole square visible and east/north distances undistorted; zooming scales
/// both axes together and panning keeps both spans, so it stays that way.
fn reference_viewport(half_diagonal: f64) -> ViewportState {
    let half_span = half_diagonal * GUIDANCE_VIEW_SCALE * std::f64::consts::FRAC_1_SQRT_2;
    ViewportState {
        x_min: -half_span,
        x_max: half_span,
        y_min: -half_span,
        y_max: half_span,
        x_mode: AxisZoomMode::Manual,
        y_mode: AxisZoomMode::Manual,
        follow: FollowMode::Free,
        x_window_size: 2.0 * half_span,
        lock_aspect_ratio: true,
    }
}

/// Closed outline of the reference square; its SW-to-NE diagonal bisects north and east.
fn reference_square(half_diagonal: f64) -> Vec<ScatterPoint> {
    let half_side = half_diagonal * std::f64::consts::FRAC_1_SQRT_2;
    [
        (-1.0, -1.0),
        (1.0, -1.0),
        (1.0, 1.0),
        (-1.0, 1.0),
        (-1.0, -1.0),
    ]
    .into_iter()
    .map(|(east, north)| {
        ScatterPoint::new(east * half_side, north * half_side).color(REFERENCE_FRAME_COLOR)
    })
    .collect()
}

/// East/north offset in global metres of `coordinate` from the frame center.
fn to_plot(center: &router::Coordinate, coordinate: &router::Coordinate) -> Option<(f64, f64)> {
    let metres_per_degree: f32 = METRES_PER_DEGREE.into();
    let metres_per_degree = f64::from(metres_per_degree);
    let east = (degrees(coordinate.longitude) - degrees(center.longitude)) * metres_per_degree;
    let north = (degrees(coordinate.latitude) - degrees(center.latitude)) * metres_per_degree;
    (east.is_finite() && north.is_finite()).then_some((east, north))
}

/// Plot position and compass heading (radians) of the Mote.
fn plot_position(
    center: &router::Coordinate,
    position: &router::Position,
) -> Option<((f64, f64), f64)> {
    let heading = degrees(position.heading).to_radians();
    heading
        .is_finite()
        .then(|| to_plot(center, &position.coordinate))
        .flatten()
        .map(|origin| (origin, heading))
}

/// East/north unit vector of a compass heading (radians, clockwise from north).
fn heading_direction(heading: f64) -> (f64, f64) {
    (heading.sin(), heading.cos())
}

/// Tip of the guidance vector. Guidance headings are relative to the Mote's compass
/// heading, so the absolute bearing is their sum; compass bearings are clockwise from north.
fn guidance_tip(
    (east, north): (f64, f64),
    heading: f64,
    guidance: &router::Guidance,
) -> Option<(f64, f64)> {
    let relative_heading: f32 = guidance.heading.into();
    let distance: f32 = guidance.distance.into();
    let bearing = heading + f64::from(relative_heading);
    let distance = f64::from(distance);
    let (east_step, north_step) = heading_direction(bearing);
    let tip = (east + distance * east_step, north + distance * north_step);
    (tip.0.is_finite() && tip.1.is_finite()).then_some(tip)
}

fn degrees(value: units::imperial::Degrees) -> f64 {
    let degrees: f32 = value.into();
    f64::from(degrees)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mote_api::work::guidance;

    fn coordinate(latitude: f32, longitude: f32) -> router::Coordinate {
        router::Coordinate {
            latitude: units::imperial::Degrees::new(latitude),
            longitude: units::imperial::Degrees::new(longitude),
        }
    }

    fn position(latitude: f32, longitude: f32, heading_degrees: f32) -> router::Position {
        router::Position {
            timestamp: units::Time::new(0.0),
            coordinate: coordinate(latitude, longitude),
            heading: units::imperial::Degrees::new(heading_degrees),
        }
    }

    fn reference_frame(global_diagonal: f32) -> router::ReferenceFrame {
        router::ReferenceFrame {
            global_center: coordinate(37.0, -121.0),
            global_diagonal: units::Length::new(global_diagonal),
            local_diagonal: units::Length::new(2.0),
            structure_side_wall_length: units::Length::new(0.2032),
        }
    }

    fn series<'a>(dataset: &'a ScatterDataset, id: &str) -> Option<&'a ScatterSeries> {
        dataset
            .series
            .iter()
            .find(|series| series.id.as_str() == format!("mote.telemetry.guidance.{id}"))
    }

    fn assert_close(actual: f64, expected: f64, tolerance: f64) {
        assert!(
            (actual - expected).abs() <= tolerance,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn view_is_a_square_sized_from_the_global_diagonal() {
        let half = half_diagonal(&reference_frame(200.0)).expect("diagonal is valid");
        let viewport = reference_viewport(half);
        let half_span = 200.0 * GUIDANCE_VIEW_SCALE / (2.0 * std::f64::consts::SQRT_2);

        assert_close(viewport.x_min, -half_span, 1e-9);
        assert_close(viewport.x_max, half_span, 1e-9);
        assert_close(viewport.y_min, -half_span, 1e-9);
        assert_close(viewport.y_max, half_span, 1e-9);
        assert_close(viewport.x_window_size, 2.0 * half_span, 1e-9);
        assert_eq!(viewport.x_mode, AxisZoomMode::Manual);
        assert_eq!(viewport.y_mode, AxisZoomMode::Manual);
        assert!(viewport.lock_aspect_ratio);
        assert!(half_diagonal(&reference_frame(0.0)).is_none());
        assert!(half_diagonal(&reference_frame(f32::NAN)).is_none());
    }

    #[test]
    fn zoom_and_pan_are_kept_until_a_different_reference_frame_is_loaded() {
        let mut context = Context::default();
        let frame = reference_frame(200.0);
        context.apply_reference_frame(&frame, 100.0);
        assert_eq!(context.viewport, reference_viewport(100.0));

        assert!(context.viewport.zoom_about(2.0, (10.0, -5.0)));
        assert!(context.viewport.pan_by(-20.0, 15.0));
        let zoomed = context.viewport;
        context.apply_reference_frame(&frame, 100.0);
        assert_eq!(context.viewport, zoomed);
        assert!(context.viewport.lock_aspect_ratio);
        assert_close(context.viewport.x_span(), context.viewport.y_span(), 1e-9);

        context.apply_reference_frame(&reference_frame(400.0), 200.0);
        assert_eq!(context.viewport, reference_viewport(200.0));
    }

    #[test]
    fn reference_square_corners_lie_on_the_diagonal() {
        let corners = reference_square(50.0);
        assert_eq!(corners.len(), 5);
        assert_eq!(corners.first(), corners.last());
        let northeast = corners[2];
        assert_close(northeast.x.hypot(northeast.y), 50.0, 1e-9);
        assert_close(northeast.x, northeast.y, 1e-9);
    }

    #[test]
    fn route_is_mapped_relative_to_the_frame_center() {
        let frame = reference_frame(200.0);
        let route: router::Route = [coordinate(37.0, -121.0), coordinate(37.0005, -120.999)]
            .into_iter()
            .collect();
        let dataset = dataset(&frame, 100.0, Some(&route), &LatestTelemetry::default());

        let route_series = series(&dataset, "route").expect("route is plotted");
        assert!(route_series.line.is_some());
        assert_eq!(route_series.points.len(), 2);
        assert_close(route_series.points[0].x, 0.0, 1e-6);
        assert_close(route_series.points[0].y, 0.0, 1e-6);
        assert_close(route_series.points[1].x, 111.3195, 1.0);
        assert_close(route_series.points[1].y, 55.659_75, 1.0);
        assert!(series(&dataset, "vector").is_none());
        assert_eq!(waiting_for(&LatestTelemetry::default()), Some("Guidance"));
    }

    #[test]
    fn guidance_vector_starts_at_the_position_and_ends_at_the_target_waypoint() {
        let frame = reference_frame(400.0);
        let target = coordinate(37.0009, -120.9992);
        let route: router::Route = [target.clone()].into_iter().collect();
        let mote_position = position(36.9995, -121.0004, 30.0);
        let guidance = guidance::Context::default()
            .update(
                units::Time::new(1.0),
                &mote_position,
                &route,
                &mut router::Status::default(),
            )
            .expect("route has a waypoint to steer to");

        let mut telemetry = LatestTelemetry::default();
        telemetry.guidance = Some(guidance);
        telemetry.position = Some(mote_position);
        let dataset = dataset(&frame, 200.0, Some(&route), &telemetry);

        let origin = series(&dataset, "position")
            .expect("position is plotted")
            .points[0];
        let vector = series(&dataset, "vector").expect("guidance vector is plotted");
        let arrow = vector.arrows[0];
        let (target_east, target_north) = to_plot(&frame.global_center, &target).unwrap();

        assert_close(arrow.origin_x, origin.x, 1e-9);
        assert_close(arrow.origin_y, origin.y, 1e-9);
        assert_close(arrow.tip_x, target_east, 0.5);
        assert_close(arrow.tip_y, target_north, 0.5);
        assert_eq!(waiting_for(&telemetry), None);
    }

    #[test]
    fn guidance_heading_is_relative_to_the_compass_heading() {
        let guidance = router::Guidance {
            timestamp: units::Time::new(0.0),
            heading: units::PlaneAngle::new(-std::f32::consts::FRAC_PI_2),
            distance: units::Length::new(10.0),
        };
        // Facing east and turning 90 degrees left points north.
        let tip = guidance_tip((1.0, 2.0), std::f64::consts::FRAC_PI_2, &guidance).unwrap();
        assert_close(tip.0, 1.0, 1e-5);
        assert_close(tip.1, 12.0, 1e-5);
    }

    #[test]
    fn guidance_vector_needs_a_position() {
        let mut telemetry = LatestTelemetry::default();
        telemetry.guidance = Some(router::Guidance::default());
        let dataset = dataset(&reference_frame(200.0), 100.0, None, &telemetry);
        assert!(series(&dataset, "vector").is_none());
        assert!(series(&dataset, "heading").is_none());
        assert!(series(&dataset, "route").is_none());
        assert!(series(&dataset, "reference_frame").is_some());
        assert_eq!(waiting_for(&telemetry), Some("Position"));
    }

    #[test]
    fn heading_is_a_fixed_length_arrow_from_the_position() {
        let mut telemetry = LatestTelemetry::default();
        telemetry.position = Some(position(37.0001, -120.9998, 90.0));
        let dataset = dataset(&reference_frame(200.0), 100.0, None, &telemetry);

        let origin = series(&dataset, "position")
            .expect("position is plotted")
            .points[0];
        let heading = series(&dataset, "heading").expect("heading is plotted");
        assert_eq!(heading.arrows.len(), 1);
        let arrow = heading.arrows[0];
        assert_eq!(arrow.screen_length, Some(HEADING_ARROW_LENGTH));
        assert_close(arrow.origin_x, origin.x, 1e-9);
        assert_close(arrow.origin_y, origin.y, 1e-9);
        // A compass heading of 90 degrees points east.
        assert_close(arrow.tip_x - arrow.origin_x, 1.0, 1e-6);
        assert_close(arrow.tip_y - arrow.origin_y, 0.0, 1e-6);
        assert!(series(&dataset, "vector").is_none(), "no guidance yet");

        let (east, north) = heading_direction(std::f64::consts::PI);
        assert_close(east, 0.0, 1e-9);
        assert_close(north, -1.0, 1e-9);
    }
}
