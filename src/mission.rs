use bee_ui::{
    UiRoot,
    prelude::{Map, lon_lat},
};

pub(crate) fn render(ui: &mut UiRoot<'_>) {
    let initial_center = lon_lat(-117.8838, 34.9054);
    ui.map(
        Map::new("mote.mission.map", initial_center)
            .fill_height()
            .initial_zoom(8.9),
    );
}
