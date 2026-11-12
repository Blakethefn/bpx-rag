use super::{ModelAction, WatchAction};

#[test]
fn model_actions_are_status_and_download() {
    assert_eq!(ModelAction::wire_names(), ["status", "download"]);
    assert_eq!(
        ModelAction::parse(Some("status")).unwrap(),
        ModelAction::Status
    );
    assert_eq!(
        ModelAction::parse(Some("download")).unwrap(),
        ModelAction::Download
    );
    assert_eq!(
        ModelAction::parse(None).unwrap_err().to_string(),
        "unknown models action ''"
    );
    assert_eq!(
        ModelAction::parse(Some("list")).unwrap_err().to_string(),
        "unknown models action 'list'"
    );
}

#[test]
fn watch_actions_are_on_off_and_status() {
    assert_eq!(WatchAction::wire_names(), ["on", "off", "status"]);
    assert_eq!(WatchAction::parse(Some("on")).unwrap(), WatchAction::On);
    assert_eq!(WatchAction::parse(Some("off")).unwrap(), WatchAction::Off);
    assert_eq!(
        WatchAction::parse(Some("status")).unwrap(),
        WatchAction::Status
    );
    assert_eq!(
        WatchAction::parse(None).unwrap_err().to_string(),
        "unknown watch action ''"
    );
    assert_eq!(
        WatchAction::parse(Some("enable")).unwrap_err().to_string(),
        "unknown watch action 'enable'"
    );
    assert_eq!(
        WatchAction::parse(Some("run")).unwrap_err().to_string(),
        "unknown watch action 'run'"
    );
}
