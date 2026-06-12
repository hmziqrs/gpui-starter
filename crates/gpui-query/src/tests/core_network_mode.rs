use crate::core::*;

#[test]
fn test_default_is_online() {
    assert_eq!(NetworkMode::default(), NetworkMode::Online);
}

#[test]
fn test_label_variants() {
    assert_eq!(NetworkMode::Online.label(), "online");
    assert_eq!(NetworkMode::Always.label(), "always");
    assert_eq!(NetworkMode::OfflineFirst.label(), "offline-first");
}

#[test]
fn test_equality() {
    assert_eq!(NetworkMode::Online, NetworkMode::Online);
    assert_eq!(NetworkMode::Always, NetworkMode::Always);
    assert_eq!(NetworkMode::OfflineFirst, NetworkMode::OfflineFirst);
    assert_ne!(NetworkMode::Online, NetworkMode::Always);
    assert_ne!(NetworkMode::Online, NetworkMode::OfflineFirst);
    assert_ne!(NetworkMode::Always, NetworkMode::OfflineFirst);
}

#[test]
fn test_serde_roundtrip() {
    for mode in [
        NetworkMode::Online,
        NetworkMode::Always,
        NetworkMode::OfflineFirst,
    ] {
        let json = serde_json::to_string(&mode).unwrap();
        let back: NetworkMode = serde_json::from_str(&json).unwrap();
        assert_eq!(mode, back);
    }
}
