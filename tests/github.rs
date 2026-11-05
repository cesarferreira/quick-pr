use quick_pr::github::*;

#[test]
fn rejects_graphql_partial_errors_instead_of_showing_incomplete_success() {
    assert!(
        parse_response(
            r#"{"data":{"viewer":{"login":"alice"}},"errors":[{"message":"Rate limit exceeded"}]}"#
        )
        .is_err()
    );
}

#[test]
fn accepts_successful_graphql_response() {
    assert_eq!(
        parse_response(r#"{"data":{"viewer":{"login":"alice"}}}"#).unwrap()["viewer"]["login"],
        "alice"
    );
}

#[test]
fn refresh_is_slower_when_hidden_and_backs_off_after_failures() {
    assert_eq!(refresh_delay(true, 0), 60);
    assert_eq!(refresh_delay(false, 0), 300);
    assert_eq!(refresh_delay(true, 1), 120);
    assert_eq!(refresh_delay(true, 6), 3600);
}
