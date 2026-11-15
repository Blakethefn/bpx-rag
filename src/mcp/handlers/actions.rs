use anyhow::{anyhow, Result};

const MODEL_ACTIONS: &[(ModelAction, &str)] = &[
    (ModelAction::Status, "status"),
    (ModelAction::Download, "download"),
];

const WATCH_ACTIONS: &[(WatchAction, &str)] = &[
    (WatchAction::On, "on"),
    (WatchAction::Off, "off"),
    (WatchAction::Status, "status"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModelAction {
    Status,
    Download,
}

impl ModelAction {
    pub(crate) fn parse(value: Option<&str>) -> Result<Self> {
        MODEL_ACTIONS
            .iter()
            .find(|(_, name)| Some(*name) == value)
            .map(|(action, _)| *action)
            .ok_or_else(|| anyhow!("unknown models action '{}'", value.unwrap_or("")))
    }

    fn as_str(self) -> &'static str {
        MODEL_ACTIONS
            .iter()
            .find(|(action, _)| *action == self)
            .map(|(_, name)| *name)
            .expect("every models action has one wire name")
    }

    pub(crate) fn wire_names() -> Vec<&'static str> {
        MODEL_ACTIONS
            .iter()
            .map(|(action, _)| action.as_str())
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WatchAction {
    On,
    Off,
    Status,
}

impl WatchAction {
    pub(crate) fn parse(value: Option<&str>) -> Result<Self> {
        WATCH_ACTIONS
            .iter()
            .find(|(_, name)| Some(*name) == value)
            .map(|(action, _)| *action)
            .ok_or_else(|| anyhow!("unknown watch action '{}'", value.unwrap_or("")))
    }

    fn as_str(self) -> &'static str {
        WATCH_ACTIONS
            .iter()
            .find(|(action, _)| *action == self)
            .map(|(_, name)| *name)
            .expect("every watch action has one wire name")
    }

    pub(crate) fn wire_names() -> Vec<&'static str> {
        WATCH_ACTIONS
            .iter()
            .map(|(action, _)| action.as_str())
            .collect()
    }
}
