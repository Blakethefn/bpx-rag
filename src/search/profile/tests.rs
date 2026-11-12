use super::SearchProfile;

#[test]
fn blank_profile_is_default_and_default_is_not_an_input() {
    assert_eq!(SearchProfile::parse(None).unwrap(), SearchProfile::Default);
    assert_eq!(
        SearchProfile::parse(Some("")).unwrap(),
        SearchProfile::Default
    );
    assert_eq!(SearchProfile::Default.as_str(), "default");
    assert_eq!(
        SearchProfile::input_names(),
        ["code_review", "debug", "planning", "vault_navigation"]
    );
    assert_eq!(
        SearchProfile::parse(Some("default")).unwrap_err().to_string(),
        "Unknown profile 'default'. Expected one of: code_review, debug, planning, vault_navigation"
    );
}

#[test]
fn explicit_profiles_round_trip_through_the_wire_table() {
    for profile in [
        SearchProfile::Default,
        SearchProfile::CodeReview,
        SearchProfile::Debug,
        SearchProfile::Planning,
        SearchProfile::VaultNavigation,
    ] {
        match profile {
            SearchProfile::Default => assert_eq!(profile.as_str(), "default"),
            SearchProfile::CodeReview => {
                assert_eq!(profile.as_str(), "code_review");
                assert_eq!(SearchProfile::parse(Some("code_review")).unwrap(), profile);
            }
            SearchProfile::Debug => {
                assert_eq!(profile.as_str(), "debug");
                assert_eq!(SearchProfile::parse(Some("debug")).unwrap(), profile);
            }
            SearchProfile::Planning => {
                assert_eq!(profile.as_str(), "planning");
                assert_eq!(SearchProfile::parse(Some("planning")).unwrap(), profile);
            }
            SearchProfile::VaultNavigation => {
                assert_eq!(profile.as_str(), "vault_navigation");
                assert_eq!(
                    SearchProfile::parse(Some("vault_navigation")).unwrap(),
                    profile
                );
            }
        }
    }
    assert_eq!(
        SearchProfile::parse(Some("review"))
            .unwrap_err()
            .to_string(),
        "Unknown profile 'review'. Expected one of: code_review, debug, planning, vault_navigation"
    );
}
