//! Tests for template

use crate::http::template::TemplateRenderer;
use minijinja::Environment;

#[tokio::test]
async fn test_minijinja_rendering() {
    let _renderer = TemplateRenderer::new(".");

    // Test that minijinja can render a simple template
    let mut env = Environment::new();
    env.add_template("test", "Hello {{name}}!").unwrap();
    let tmpl = env.get_template("test").unwrap();
    let result = tmpl.render(serde_json::json!({"name": "World"})).unwrap();
    assert_eq!(result, "Hello World!");
}
