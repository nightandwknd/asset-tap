//! Template registry with loading and discovery.

use super::config::TemplateDefinition;
use super::interpolation::interpolate;
use crate::constants::files;
use crate::settings::{config_dir, is_dev_mode};
use anyhow::{Result, anyhow};
use include_dir::{Dir, include_dir};
use indexmap::IndexMap;
use std::collections::HashMap;
use std::path::PathBuf;

/// Embedded template configs (all *.yaml files from templates/ directory).
static EMBEDDED_TEMPLATES: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../templates");

/// Error that occurred while loading a template.
#[derive(Clone, Debug)]
pub struct TemplateLoadError {
    /// Path to the template file that failed to load.
    pub path: String,
    /// Error message.
    pub error: String,
    /// Error kind for categorization.
    pub kind: TemplateErrorKind,
}

/// Kind of template loading error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TemplateErrorKind {
    /// File could not be read (IO error).
    FileRead,
    /// YAML parsing failed (syntax error).
    YamlParse,
    /// Template validation failed (missing required fields, invalid structure).
    Validation,
}

/// Registry of available templates.
///
/// Templates are loaded from:
/// 1. Embedded defaults (written to user directory if missing)
/// 2. User directory (.dev/templates in dev mode, ~/.config/asset-tap/templates in release)
///
/// User templates override embedded ones.
pub struct TemplateRegistry {
    templates: IndexMap<String, TemplateDefinition>,
    /// Errors that occurred during loading (non-fatal).
    pub load_errors: Vec<TemplateLoadError>,
}

impl TemplateRegistry {
    /// Create a new template registry and load all templates.
    pub fn new() -> Self {
        let mut registry = Self {
            templates: IndexMap::new(),
            load_errors: Vec::new(),
        };

        // Ensure embedded templates exist in user dir (write if missing)
        if let Err(e) = ensure_default_templates_exist() {
            tracing::warn!("Failed to write default templates: {}", e);
        }

        // Discover and load from user directory
        registry.discover_templates_from_dir(&get_user_templates_dir());

        tracing::info!("Loaded {} templates", registry.templates.len());

        registry
    }

    /// Discover and load templates from a directory.
    fn discover_templates_from_dir(&mut self, dir: &PathBuf) {
        if !dir.exists() {
            tracing::debug!("Template directory does not exist: {:?}", dir);
            return;
        }

        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) => {
                tracing::warn!("Failed to read template directory {:?}: {}", dir, e);
                return;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();

            // Only process .yaml and .yml files
            let ext = path.extension().and_then(|s| s.to_str());
            if ext != Some("yaml") && ext != Some("yml") {
                continue;
            }

            match TemplateDefinition::from_yaml_file(&path) {
                Ok(mut template) => {
                    // Check if this is an embedded template (exists in templates/ dir)
                    template.is_builtin = is_embedded_template(&template.id);

                    tracing::info!("Loaded template: {} ({})", template.name, template.id);
                    self.templates.insert(template.id.clone(), template);
                }
                Err(e) => {
                    // Categorize the error
                    let error_str = e.to_string();
                    let kind = if error_str.contains("Failed to read") {
                        TemplateErrorKind::FileRead
                    } else if error_str.contains("Failed to parse") || error_str.contains("YAML") {
                        TemplateErrorKind::YamlParse
                    } else {
                        TemplateErrorKind::Validation
                    };

                    tracing::warn!(
                        "Failed to load template from {:?}: {} ({:?})",
                        path,
                        error_str,
                        kind
                    );
                    self.load_errors.push(TemplateLoadError {
                        path: path.display().to_string(),
                        error: error_str,
                        kind,
                    });
                }
            }
        }
    }

    /// Get a template by ID.
    pub fn get(&self, id: &str) -> Option<&TemplateDefinition> {
        self.templates.get(id)
    }

    /// List all available templates.
    pub fn list(&self) -> Vec<&TemplateDefinition> {
        self.templates.values().collect()
    }

    /// Get the number of loaded templates.
    pub fn count(&self) -> usize {
        self.templates.len()
    }

    /// Check if there were any load errors.
    pub fn has_load_errors(&self) -> bool {
        !self.load_errors.is_empty()
    }

    /// Apply a template with the given variables.
    ///
    /// # Arguments
    ///
    /// * `id` - Template ID
    /// * `vars` - Map of variable names to values
    ///
    /// # Returns
    ///
    /// The interpolated template string, or an error if the template doesn't exist.
    pub fn apply(&self, id: &str, vars: &HashMap<String, String>) -> Result<String> {
        let template = self
            .get(id)
            .ok_or_else(|| anyhow!("Template not found: {}", id))?;

        interpolate(&template.template, vars)
    }

    /// Add a new template to the registry and save it to disk.
    ///
    /// Returns an error if:
    /// - A template with the same ID already exists
    /// - The template fails validation
    /// - Writing to disk fails
    pub fn add(&mut self, template: TemplateDefinition) -> Result<()> {
        // Check for duplicate ID
        if self.templates.contains_key(&template.id) {
            return Err(anyhow!("Template '{}' already exists", template.id));
        }

        // Validate
        template.validate()?;

        // Save to disk
        let templates_dir = get_user_templates_dir();
        let filename = format!("{}.yaml", template.id);
        let path = templates_dir.join(&filename);

        template.save_to_yaml_file(&path)?;

        // Add to registry
        self.templates.insert(template.id.clone(), template);

        Ok(())
    }

    /// Delete a template by ID.
    ///
    /// Returns an error if:
    /// - Trying to delete a builtin template
    /// - Template doesn't exist
    /// - File deletion fails
    pub fn delete(&mut self, id: &str) -> Result<()> {
        let template = self
            .get(id)
            .ok_or_else(|| anyhow!("Template '{}' not found", id))?;

        // Prevent deleting builtins
        if template.is_builtin {
            return Err(anyhow!("Cannot delete builtin template '{}'", id));
        }

        // Delete file
        if let Some(path) = &template.source_path
            && path.exists()
        {
            std::fs::remove_file(path)
                .map_err(|e| anyhow!("Failed to delete template file: {}", e))?;
        }

        // Remove from registry
        self.templates.shift_remove(id);

        Ok(())
    }
}

impl Default for TemplateRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Get the user templates directory.
fn get_user_templates_dir() -> PathBuf {
    if is_dev_mode() {
        PathBuf::from(files::dev_dirs::TEMPLATES)
    } else {
        config_dir().join(files::config::TEMPLATES_DIR)
    }
}

/// Check if a template ID corresponds to an embedded template.
///
/// Returns true if the template exists in the embedded templates directory.
fn is_embedded_template(id: &str) -> bool {
    let filename_yaml = format!("{}.yaml", id);
    let filename_yml = format!("{}.yml", id);

    EMBEDDED_TEMPLATES.files().any(|f| {
        if let Some(name) = f.path().file_name() {
            let name_str = name.to_string_lossy();
            name_str == filename_yaml || name_str == filename_yml
        } else {
            false
        }
    })
}

/// Ensure default embedded templates exist in the user directory.
///
/// This dynamically discovers all templates from the embedded templates/ directory
/// and writes them to disk if they don't already exist.
fn ensure_default_templates_exist() -> Result<()> {
    let templates_dir = get_user_templates_dir();
    std::fs::create_dir_all(&templates_dir)
        .map_err(|e| anyhow!("Failed to create templates directory: {}", e))?;

    // Iterate through all embedded template files
    for file in EMBEDDED_TEMPLATES.files() {
        // Only process .yaml and .yml files
        let path = file.path();
        let ext = path.extension().and_then(|s| s.to_str());
        if !matches!(ext, Some("yaml") | Some("yml")) {
            continue;
        }

        // Skip if file is in archive/ subdirectory
        if path.components().any(|c| c.as_os_str() == "archive") {
            tracing::debug!("Skipping archived template: {:?}", path);
            continue;
        }

        // Get filename and target path
        let filename = match path.file_name() {
            Some(name) => name,
            None => continue,
        };
        let target_path = templates_dir.join(filename);

        // Get file contents
        let contents = match file.contents_utf8() {
            Some(c) => c,
            None => {
                tracing::warn!("Template file {:?} is not valid UTF-8", path);
                continue;
            }
        };

        // Content-compare write: creates new, overwrites (with .bak) when bytes differ, or skips.
        crate::config_sync::write_with_backup(&target_path, contents, "template")
            .map_err(|e| anyhow!("Failed to write template {:?}: {}", filename, e))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_registry_creation() {
        let registry = TemplateRegistry::new();
        // Should have at least the humanoid template
        assert!(registry.count() > 0);
    }

    #[test]
    fn test_get_template() {
        let registry = TemplateRegistry::new();
        assert!(registry.get("humanoid").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_list_templates() {
        let registry = TemplateRegistry::new();
        let templates = registry.list();
        assert!(!templates.is_empty());
    }

    #[test]
    fn test_apply_template() {
        let registry = TemplateRegistry::new();
        let mut vars = HashMap::new();
        vars.insert("description".to_string(), "a cowboy ninja".to_string());

        let result = registry.apply("humanoid", &vars);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("cowboy ninja"));
    }

    #[test]
    fn test_apply_nonexistent_template() {
        let registry = TemplateRegistry::new();
        let vars = HashMap::new();

        let result = registry.apply("nonexistent", &vars);
        assert!(result.is_err());
    }

    #[test]
    fn test_error_handling_corrupt_yaml() {
        // Create a corrupt YAML file
        let templates_dir = get_user_templates_dir();
        fs::create_dir_all(&templates_dir).ok();
        let corrupt_path = templates_dir.join("test_corrupt.yaml");
        fs::write(&corrupt_path, "id: test\nname: broken\ninvalid: [unclosed").ok();

        // Load registry
        let registry = TemplateRegistry::new();

        // Should have captured the error
        let has_yaml_parse_error = registry
            .load_errors
            .iter()
            .any(|e| e.kind == TemplateErrorKind::YamlParse);
        assert!(has_yaml_parse_error);

        // Clean up
        fs::remove_file(&corrupt_path).ok();
    }

    #[test]
    fn test_error_handling_validation_failure() {
        // Create a YAML file that parses but fails validation
        let templates_dir = get_user_templates_dir();
        fs::create_dir_all(&templates_dir).ok();
        let invalid_path = templates_dir.join("test_invalid.yaml");
        fs::write(
            &invalid_path,
            "id: \"\"\nname: Invalid\ndescription: Empty ID\ntemplate: Test",
        )
        .ok();

        // Load registry
        let registry = TemplateRegistry::new();

        // Should have captured the validation error
        let has_validation_error = registry
            .load_errors
            .iter()
            .any(|e| e.kind == TemplateErrorKind::Validation);
        assert!(has_validation_error);

        // Clean up
        fs::remove_file(&invalid_path).ok();
    }
}
