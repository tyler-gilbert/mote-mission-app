use std::time::Instant;

use crate::{
    transport::{TransportWorker, WorkerSnapshot},
    ui::ControlPanel,
};

pub(crate) struct Context {
    pub(crate) worker: TransportWorker,

    pub(crate) device_refresh_until: Option<Instant>,
    pub(crate) snapshot: WorkerSnapshot,
    pub(crate) last_error: Option<String>,
    pub(crate) active_panel: ControlPanel,
    pub(crate) refreshing_devices: bool,
}

#[derive(Debug, PartialEq)]
pub(crate) enum UiAction {
    SelectPanel(ControlPanel),
    Refresh,
    SelectDevice(String),
    Connect(String),
    Disconnect,
}
