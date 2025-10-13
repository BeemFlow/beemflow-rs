use super::*;
use crate::model::{OAuthCredential, Trigger};
use chrono::Utc;

#[test]
fn test_flow_deserialization() {
    let yaml = r#"
name: hello
description: Hello world flow
on: cli.manual
steps:
  - id: greet
    use: core.echo
    with:
      text: "Hello, world!"
"#;

    let flow: Flow = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(flow.name, "hello");
    assert_eq!(flow.steps.len(), 1);
    assert_eq!(flow.steps[0].id, "greet");
}

#[test]
fn test_trigger_includes() {
    let single = Trigger::Single("cli.manual".to_string());
    assert!(single.includes("cli.manual"));
    assert!(!single.includes("http.request"));

    let multiple = Trigger::Multiple(vec!["cli.manual".to_string(), "schedule.cron".to_string()]);
    assert!(multiple.includes("cli.manual"));
    assert!(multiple.includes("schedule.cron"));
    assert!(!multiple.includes("http.request"));
}

#[test]
fn test_oauth_credential_expired() {
    let mut cred = OAuthCredential {
        id: "test".to_string(),
        provider: "google".to_string(),
        integration: "sheets".to_string(),
        access_token: "token".to_string(),
        refresh_token: None,
        expires_at: Some(Utc::now() - chrono::Duration::hours(1)),
        scope: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    assert!(cred.is_expired());

    cred.expires_at = Some(Utc::now() + chrono::Duration::hours(1));
    assert!(!cred.is_expired());
}
