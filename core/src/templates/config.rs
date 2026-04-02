//! Template configuration and loading from YAML files.

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A template definition loaded from YAML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateDefinition {
    /// Config file version for automatic upgrades. Missing = version 0.
    #[serde(default)]
    pub config_version: u32,

    /// Unique template identifier.
    pub id: String,

    /// Human-readable name.
    pub name: String,

    /// Description of what this template is optimized for.
    pub description: String,

    /// Optional category for grouping templates.
    #[serde(default)]
    pub category: Option<String>,

    /// The template string with ${variable} placeholders.
    pub template: String,

    /// Variables that can be used in the template.
    #[serde(default)]
    pub variables: Vec<TemplateVariable>,

    /// Example descriptions for this template.
    #[serde(default)]
    pub examples: Vec<String>,

    // Runtime fields (not in YAML, set during load)
    /// True for built-in templates, false for user-created.
    #[serde(skip)]
    pub is_builtin: bool,

    /// Path to the source YAML file.
    #[serde(skip)]
    pub source_path: Option<PathBuf>,
}

/// A template variable definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateVariable {
    /// Variable name (e.g., "description").
    pub name: String,

    /// Optional description of what this variable is for.
    #[serde(default)]
    pub description: Option<String>,

    /// Whether this variable is required.
    #[serde(default = "default_true")]
    pub required: bool,
}

fn default_true() -> bool {
    true
}

impl TemplateDefinition {
    /// Validate the template definition.
    ///
    /// Checks:
    /// - ID, name, and template are not empty
    /// - All required variables are present in the template string
    pub fn validate(&self) -> Result<()> {
        if self.id.trim().is_empty() {
            return Err(anyhow!("Template ID cannot be empty"));
        }
        if self.name.trim().is_empty() {
            return Err(anyhow!("Template name cannot be empty"));
        }
        if self.template.is_empty() {
            return Err(anyhow!("Template string cannot be empty"));
        }

        // Check all required variables are present in template
        for var in &self.variables {
            if var.required {
                let placeholder = format!("${{{}}}", var.name);
                if !self.template.contains(&placeholder) {
                    return Err(anyhow!(
                        "Template '{}' missing required variable '{}'",
                        self.id,
                        var.name
                    ));
                }
            }
        }

        Ok(())
    }

    /// Load a template definition from a YAML file.
    ///
    /// This parses the YAML, validates the template, and sets the source path.
    pub fn from_yaml_file(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)
            .map_err(|e| anyhow!("Failed to read template file: {}", e))?;

        let mut template: TemplateDefinition = serde_yaml_ng::from_str(&contents)
            .map_err(|e| anyhow!("Failed to parse template YAML: {}", e))?;

        template.validate()?;
        template.source_path = Some(path.to_path_buf());

        Ok(template)
    }

    /// Save this template definition to a YAML file.
    pub fn save_to_yaml_file(&self, path: &Path) -> Result<()> {
        // Create parent directory if needed
        if let Some(parent) = path.parent()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent)
                .map_err(|e| anyhow!("Failed to create template directory: {}", e))?;
        }

        let yaml = serde_yaml_ng::to_string(self)
            .map_err(|e| anyhow!("Failed to serialize template: {}", e))?;

        std::fs::write(path, yaml).map_err(|e| anyhow!("Failed to write template file: {}", e))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_empty_id() {
        let template = TemplateDefinition {
            config_version: 0,
            id: "".to_string(),
            name: "Test".to_string(),
            description: "Test".to_string(),
            category: None,
            template: "Test ${description}".to_string(),
            variables: vec![],
            examples: vec![],
            is_builtin: false,
            source_path: None,
        };

        assert!(template.validate().is_err());
    }

    #[test]
    fn test_validate_missing_required_variable() {
        let template = TemplateDefinition {
            config_version: 0,
            id: "test".to_string(),
            name: "Test".to_string(),
            description: "Test".to_string(),
            category: None,
            template: "Test prompt".to_string(), // Missing ${description}
            variables: vec![TemplateVariable {
                name: "description".to_string(),
                description: None,
                required: true,
            }],
            examples: vec![],
            is_builtin: false,
            source_path: None,
        };

        assert!(template.validate().is_err());
    }

    #[test]
    fn test_validate_valid_template() {
        let template = TemplateDefinition {
            config_version: 0,
            id: "test".to_string(),
            name: "Test".to_string(),
            description: "Test".to_string(),
            category: None,
            template: "Test ${description}".to_string(),
            variables: vec![TemplateVariable {
                name: "description".to_string(),
                description: None,
                required: true,
            }],
            examples: vec![],
            is_builtin: false,
            source_path: None,
        };

        assert!(template.validate().is_ok());
    }
}
