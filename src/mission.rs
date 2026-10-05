use bee_ui::{
    UiRoot,
    prelude::{Map, lon_lat},
};

#[derive(Default)]
pub(crate) struct Context;

pub(crate) fn render(ui: &mut UiRoot<'_>, _context: &mut Context) {
    let initial_center = lon_lat(-117.8838, 34.9054);
    ui.map(
        Map::new("mote.mission.map", initial_center)
            .fill_height()
            .initial_zoom(8.9),
    );
}
