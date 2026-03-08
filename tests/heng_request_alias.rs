use continuum::mcp::types::HengRequest;

#[test]
fn should_parse_options_alias_for_heng_request() {
    let request: HengRequest = serde_json::from_value(serde_json::json!({
        "message": "请选择",
        "options": ["A", "B"]
    }))
    .expect("should parse options alias");

    assert_eq!(request.predefined_options, vec!["A", "B"]);
    assert!(request.is_markdown);
}

#[test]
fn should_parse_predefined_options_for_heng_request() {
    let request: HengRequest = serde_json::from_value(serde_json::json!({
        "message": "请选择",
        "predefined_options": ["A", "B"]
    }))
    .expect("should parse predefined_options");

    assert_eq!(request.predefined_options, vec!["A", "B"]);
    assert!(request.is_markdown);
}
