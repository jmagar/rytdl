use super::*;

#[test]
fn presence_masks_a_set_value_and_never_leaks_it() {
    // The whole point of the diagnostic: a populated secret is reported as
    // present without its contents ever appearing in the output. This guards
    // against a future edit that prints the real token.
    let secret = Some("supersecret".to_string());
    let rendered = presence(&secret);

    assert_eq!(rendered, "set (••••)");
    assert!(
        !rendered.contains("supersecret"),
        "presence() must never leak the underlying value"
    );
}

#[test]
fn presence_reports_unset_for_none() {
    assert_eq!(presence(&None::<String>), "not set");
}

#[test]
fn yes_no_renders_both_arms() {
    assert_eq!(yes_no(true), "yes");
    assert_eq!(yes_no(false), "no");
}
