//! Tests for cron

use super::*;

#[test]
fn test_shell_quote() {
    assert_eq!(shell_quote("simple"), "'simple'");
    assert_eq!(shell_quote("with'quote"), "'with'\\''quote'");
    assert_eq!(shell_quote("multiple'quotes'here"), "'multiple'\\''quotes'\\''here'");
}

#[test]
fn test_has_schedule_cron_trigger() {
    let flow = Flow {
        cron: Some("0 * * * *".to_string()),
        ..Default::default()
    };

    assert!(CronManager::has_schedule_cron_trigger(&flow));

    let flow_no_cron = Flow {
        cron: None,
        ..Default::default()
    };
    assert!(!CronManager::has_schedule_cron_trigger(&flow_no_cron));
}

#[tokio::test]
async fn test_cron_manager_creation() {
    let manager = CronManager::new("http://localhost:3000".to_string(), Some("secret".to_string()));
    assert_eq!(manager.server_url, "http://localhost:3000");
    assert_eq!(manager.cron_secret, Some("secret".to_string()));
}
