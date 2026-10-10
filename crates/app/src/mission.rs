use bee_ui::{
    UiRoot,
    prelude::{Map, MapRoute, Position, lon_lat},
};
use eframe::egui;

use crate::settings;

const FALLBACK_CENTER: (f64, f64) = (34.9054, -117.8838);
const EARTH_RADIUS_METERS: f64 = 6_371_008.8;

#[derive(Default)]
pub(crate) struct Context;

pub(crate) fn render(ui: &mut UiRoot<'_>, _context: &mut Context, settings: &settings::Context) {
    let global_center = settings.global_center();
    let (latitude, longitude) = global_center.unwrap_or(FALLBACK_CENTER);
    let center = lon_lat(longitude, latitude);
    let square = global_center
        .zip(settings.global_diagonal_meters())
        .map(|((latitude, longitude), diagonal)| square_corners(latitude, longitude, diagonal));
    let map_route = settings.route().map(|route| {
        MapRoute::new(route.iter().map(|coordinate| {
            let latitude: f32 = coordinate.latitude.into();
            let longitude: f32 = coordinate.longitude.into();
            lon_lat(f64::from(longitude), f64::from(latitude))
        }))
        .color(egui::Color32::MAGENTA)
        .width(3.0)
    });
    let mut map = Map::new("mote.mission.map", center)
        .fill_height()
        .initial_zoom(8.9);
    if let Some(route) = map_route {
        map = map.route(route);
    }

    ui.map_with_overlay(map, |overlay_ui, response, projector| {
        if let Some(corners) = &square {
            let points = corners
                .iter()
                .map(|corner| {
                    let projected = projector.project(corner.clone());
                    egui::pos2(projected.x, projected.y)
                })
                .collect();
            overlay_ui
                .painter()
                .with_clip_rect(response.rect)
                .add(egui::Shape::closed_line(
                    points,
                    egui::Stroke::new(2.0, egui::Color32::from_rgb(40, 120, 240)),
                ));
        }
    });
}

fn square_corners(latitude: f64, longitude: f64, diagonal_meters: f64) -> [Position; 4] {
    [45.0, 135.0, 225.0, 315.0]
        .map(|bearing| destination(latitude, longitude, diagonal_meters / 2.0, bearing))
}

fn destination(
    latitude: f64,
    longitude: f64,
    distance_meters: f64,
    bearing_degrees: f64,
) -> Position {
    let latitude = latitude.to_radians();
    let longitude = longitude.to_radians();
    let bearing = bearing_degrees.to_radians();
    let angular_distance = distance_meters / EARTH_RADIUS_METERS;

    let destination_latitude = (latitude.sin() * angular_distance.cos()
        + latitude.cos() * angular_distance.sin() * bearing.cos())
    .asin();
    let destination_longitude = longitude
        + (bearing.sin() * angular_distance.sin() * latitude.cos())
            .atan2(angular_distance.cos() - latitude.sin() * destination_latitude.sin());

    lon_lat(
        destination_longitude.to_degrees(),
        destination_latitude.to_degrees(),
    )
}
