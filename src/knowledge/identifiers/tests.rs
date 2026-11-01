use super::*;

fn words(identifier: &str) -> Vec<String> {
    split_identifier(identifier)
}

#[test]
fn splits_camel_case_at_lowercase_to_uppercase_boundaries() {
    assert_eq!(words("getUserById"), ["get", "user", "by", "id"]);
}

#[test]
fn splits_pascal_case_into_lowercase_words() {
    assert_eq!(
        words("CalculateShippingPrice"),
        ["calculate", "shipping", "price"]
    );
}

#[test]
fn splits_snake_case_on_underscores() {
    assert_eq!(words("get_user_by_id"), ["get", "user", "by", "id"]);
}

#[test]
fn splits_kebab_case_on_hyphens() {
    assert_eq!(words("blue-green-deploy"), ["blue", "green", "deploy"]);
}

#[test]
fn keeps_an_acronym_together_and_splits_before_the_next_word() {
    assert_eq!(words("HTTPServer"), ["http", "server"]);
    assert_eq!(words("parseHTTPResponse"), ["parse", "http", "response"]);
}

#[test]
fn does_not_split_digits_from_letters() {
    assert_eq!(words("sha256Hash"), ["sha256", "hash"]);
}

#[test]
fn single_word_identifiers_split_to_themselves() {
    assert_eq!(words("HTTP"), ["http"]);
    assert_eq!(words("content"), ["content"]);
}

#[test]
fn identifier_words_extracts_compound_identifiers_in_source_order() {
    assert_eq!(
        identifier_words("export function getUserById(id) { return store; }"),
        "get user by id"
    );
}

#[test]
fn identifier_words_emits_each_distinct_identifier_once() {
    assert_eq!(
        identifier_words("getUserById(); getUserById(); parseHTTPResponse()"),
        "get user by id parse http response"
    );
}

#[test]
fn identifier_words_skips_plain_words_and_single_word_identifiers() {
    assert_eq!(identifier_words("plain words only, and HTTP"), "");
}

#[test]
fn identifier_words_skips_numbers_such_as_dates() {
    assert_eq!(identifier_words("released 2026-11-01"), "");
}
