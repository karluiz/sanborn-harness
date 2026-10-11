#[test]
fn greeting_contains_name() {
    assert_eq!(format!("Hello, {}!", "pond"), "Hello, pond!");
}
