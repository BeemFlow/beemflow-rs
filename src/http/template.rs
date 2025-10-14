//! HTML template rendering for OAuth consent flows
//!
//! Provides template rendering using minijinja with full Jinja2 syntax support
//! including loops, conditionals, filters, etc.

use crate::{BeemFlowError, Result};
use minijinja::Environment;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Template renderer for HTML pages using minijinja
#[derive(Clone)]
pub struct TemplateRenderer {
    template_dir: PathBuf,
    env: Arc<Environment<'static>>,
    templates: HashMap<String, String>,
}

impl TemplateRenderer {
    /// Create a new template renderer with minijinja
    pub fn new<P: AsRef<Path>>(template_dir: P) -> Self {
        let mut env = Environment::new();
        // Auto-escape HTML for security
        env.set_auto_escape_callback(|_| minijinja::AutoEscape::Html);

        Self {
            template_dir: template_dir.as_ref().to_path_buf(),
            env: Arc::new(env),
            templates: HashMap::new(),
        }
    }

    /// Load a template from file into cache
    pub async fn load_template(&mut self, name: &str, filename: &str) -> Result<()> {
        // Validate filename to prevent path traversal attacks
        if filename.contains("..") || filename.starts_with('/') || filename.contains('\\') {
            return Err(BeemFlowError::validation(
                "Invalid template filename: path traversal not allowed",
            ));
        }

        let path = self.template_dir.join(filename);

        // Canonicalize paths to prevent traversal after join
        let canonical_path = tokio::fs::canonicalize(&path)
            .await
            .map_err(|e| BeemFlowError::config(format!("Invalid template path: {}", e)))?;

        let canonical_template_dir = tokio::fs::canonicalize(&self.template_dir)
            .await
            .map_err(|e| BeemFlowError::config(format!("Invalid template directory: {}", e)))?;

        // Ensure the resolved path is within the template directory
        if !canonical_path.starts_with(&canonical_template_dir) {
            return Err(BeemFlowError::validation(
                "Template path outside allowed directory",
            ));
        }

        let content = tokio::fs::read_to_string(&canonical_path)
            .await
            .map_err(|e| {
                BeemFlowError::config(format!("Failed to load template {}: {}", filename, e))
            })?;

        // Cache the template content
        self.templates.insert(name.to_string(), content);

        Ok(())
    }

    /// Load all OAuth templates (embedded in binary)
    pub async fn load_oauth_templates(&mut self) -> Result<()> {
        // Embed templates in binary for portability
        self.templates.insert(
            "consent".to_string(),
            include_str!("../../static/oauth/consent.html").to_string(),
        );
        self.templates.insert(
            "provider_auth".to_string(),
            include_str!("../../static/oauth/provider_auth.html").to_string(),
        );
        self.templates.insert(
            "success".to_string(),
            include_str!("../../static/oauth/success.html").to_string(),
        );
        self.templates.insert(
            "providers".to_string(),
            include_str!("../../static/oauth/providers.html").to_string(),
        );

        Ok(())
    }

    /// Render a template with JSON data (minijinja-powered)
    pub fn render_json(&self, name: &str, data: &serde_json::Value) -> Result<String> {
        let template_content = self
            .templates
            .get(name)
            .ok_or_else(|| BeemFlowError::config(format!("Template '{}' not found", name)))?;

        self.env.render_str(template_content, data).map_err(|e| {
            BeemFlowError::config(format!("Failed to render template '{}': {}", name, e))
        })
    }
}

// No longer need create_default_templates - all templates are loaded from files using minijinja
