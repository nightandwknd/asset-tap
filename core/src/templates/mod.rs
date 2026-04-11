//! Template system for prompt generation.
//!
//! This module provides a YAML-based template system for generating optimized prompts
//! from user input. Like the provider system, templates follow a **data-driven architecture**
//! where templates are loaded from configuration files.
//!
//! # Overview
//!
//! Templates use variable interpolation with `${variable}` syntax to create structured
//! prompts. For example: `"A detailed ${description} in ${style} style"`.
//!
//! # Template Locations
//!
//! Templates are loaded in this order (later sources override earlier ones):
//!
//! 1. **Embedded templates**: `templates/*.yaml` compiled into binary
//! 2. **User templates**: `.dev/templates/` (dev) or `~/.config/asset-tap/templates/` (release)
//!
//! # Quick Start
//!
//! ```
//! use asset_tap_core::templates::{apply_template, create_template, list_templates};
//!
//! // List available templates
//! let templates = list_templates();
//! println!("Available templates: {:?}", templates);
//!
//! // Apply a template
//! if let Some(prompt) = apply_template("character", "a robot warrior") {
//!     println!("Generated prompt: {}", prompt);
//! }
//! ```
//!
//! # See Also
//!
//! - [`config::TemplateDefinition`] - Template YAML configuration format
//! - [`interpolation::interpolate`] - Variable interpolation implementation
//! - [`registry::TemplateRegistry`] - Template loading and management

pub mod config;
pub mod interpolation;
pub mod registry;

// Re-export main types
pub use config::{TemplateDefinition, TemplateVariable};
pub use interpolation::interpolate;
pub use registry::{TemplateErrorKind, TemplateLoadError, TemplateRegistry};

use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

/// Global template registry.
pub static REGISTRY: LazyLock<RwLock<TemplateRegistry>> =
    LazyLock::new(|| RwLock::new(TemplateRegistry::new()));

/// Apply a named template to a description.
///
/// Uses the global template registry to find and apply the template.
/// Templates use variable interpolation with `${variable}` syntax.
///
/// # Arguments
///
/// * `name` - Template ID (e.g., "humanoid", "character")
/// * `description` - The description to pass to the template's `${description}` variable
///
/// # Returns
///
/// The formatted prompt, or `None` if the template doesn't exist.
///
/// # Examples
///
/// ```
/// use asset_tap_core::templates::apply_template;
///
/// // Apply a template (returns None if template doesn't exist)
/// if let Some(prompt) = apply_template("character", "a futuristic robot") {
///     println!("Generated prompt: {}", prompt);
/// }
/// ```
///
/// Using a non-existent template returns None:
///
/// ```
/// use asset_tap_core::templates::apply_template;
///
/// let prompt = apply_template("nonexistent", "test");
/// assert!(prompt.is_none());
/// ```
pub fn apply_template(name: &str, description: &str) -> Option<String> {
    let mut vars = HashMap::new();
    vars.insert("description".to_string(), description.to_string());

    REGISTRY.read().unwrap().apply(name, &vars).ok()
}

/// List all available template IDs.
pub fn list_templates() -> Vec<String> {
    REGISTRY
        .read()
        .unwrap()
        .list()
        .iter()
        .map(|t| t.id.clone())
        .collect()
}

/// Check if a template exists.
pub fn template_exists(name: &str) -> bool {
    REGISTRY.read().unwrap().get(name).is_some()
}

/// Generate a template ID from a name (slugify).
///
/// Converts to lowercase, replaces whitespace with hyphens, removes non-alphanumeric characters,
/// and collapses consecutive hyphens.
///
/// # Examples
///
/// ```
/// use asset_tap_core::templates::slugify;
///
/// assert_eq!(slugify("My Template"), "my-template");
/// assert_eq!(slugify("Sci-Fi Character!"), "sci-fi-character");
/// assert_eq!(slugify("  Spaced  Out  "), "spaced-out");
/// ```
pub fn slugify(name: &str) -> String {
    let mut result = name
        .trim()
        .to_lowercase()
        .replace(char::is_whitespace, "-")
        .replace(|c: char| !c.is_ascii_alphanumeric() && c != '-', "");
    // Collapse consecutive hyphens
    while result.contains("--") {
        result = result.replace("--", "-");
    }
    result
}

/// Create a new template from user input.
///
/// This is a convenience function for creating templates from the GUI/CLI.
/// It automatically generates an ID from the name and sets up the standard structure.
///
/// # Arguments
///
/// * `name` - Human-readable template name
/// * `description` - What this template is optimized for
/// * `template_string` - The template with `${description}` placeholder
/// * `category` - Optional category for organization ("character", "prop", "environment", "general")
///
/// # Returns
///
/// Result with the template ID if successful, or error message.
///
/// # Examples
///
/// ```no_run
/// use asset_tap_core::templates::create_template;
///
/// // Create a custom vehicle template
/// let template_id = create_template(
///     "Vehicle",
///     "Optimized for vehicles and transportation",
///     "A highly detailed ${description}, trending on ArtStation",
///     Some("prop".to_string())
/// )?;
///
/// println!("Created template with ID: {}", template_id);
/// # Ok::<(), String>(())
/// ```
///
/// Templates must have unique names:
///
/// ```no_run
/// # use asset_tap_core::templates::create_template;
/// // This will fail if a template already exists
/// let result = create_template("Character", "...", "...", None);
/// assert!(result.is_err());
/// # Ok::<(), String>(())
/// ```
pub fn create_template(
    name: &str,
    description: &str,
    template_string: &str,
    category: Option<String>,
) -> Result<String, String> {
    // Generate ID from name
    let id = slugify(name);

    if id.is_empty() {
        return Err("Template name cannot be empty".to_string());
    }

    // Create template definition
    let template = TemplateDefinition {
        id: id.clone(),
        name: name.trim().to_string(),
        description: description.trim().to_string(),
        category,
        template: template_string.to_string(),
        variables: vec![TemplateVariable {
            name: "description".to_string(),
            description: Some("Character or object description".to_string()),
            required: true,
        }],
        examples: vec![],
        is_builtin: false,
        source_path: None,
    };

    // Acquire write lock for atomic check-and-insert (avoids TOCTOU race)
    let mut registry = REGISTRY
        .write()
        .map_err(|e| format!("Failed to lock template registry: {}", e))?;

    // Use registry.add() which validates, saves to disk, and inserts in one step
    registry.add(template).map_err(|e| e.to_string())?;

    Ok(id)
}

/// Delete a custom template by ID.
///
/// Returns an error if:
/// - Trying to delete a builtin template
/// - Template doesn't exist
pub fn delete_custom_template(id: &str) -> Result<(), String> {
    let mut registry = REGISTRY
        .write()
        .map_err(|e| format!("Failed to lock template registry: {}", e))?;

    registry.delete(id).map_err(|e| e.to_string())
}

/// Get a template definition by ID.
///
/// Returns the template metadata including the template string.
pub fn get_template_definition(id: &str) -> Option<TemplateDefinition> {
    REGISTRY.read().unwrap().get(id).cloned()
}

/// Example character descriptions for testing.
pub const EXAMPLE_CHARACTERS: &[&str] = &[
    "a cowboy ninja with a leather duster, bandana mask, and dual katanas on the back",
    "a steampunk robot knight with brass armor and glowing eyes",
    "a muscular orc warrior with green skin and tribal tattoos",
    "a sleek android with white chassis and orange accents",
    "a hooded assassin in dark leather with hidden blades",
    "a cyborg boxer with mechanical arms and a mohawk",
    "a masked luchador wrestler in colorful spandex",
    "a ghostly spirit fighter with translucent blue skin",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_apply_template() {
        let result = apply_template("humanoid", "a knight");
        assert!(result.is_some());
        let prompt = result.unwrap();
        assert!(prompt.contains("a knight"));
        assert!(prompt.contains("T-pose"));

        let result = apply_template("nonexistent", "test");
        assert!(result.is_none());
    }

    #[test]
    fn test_list_templates() {
        let templates = list_templates();
        assert!(templates.contains(&"humanoid".to_string()));
    }

    #[test]
    fn test_template_exists() {
        assert!(template_exists("humanoid"));
        assert!(!template_exists("nonexistent"));
    }

    #[test]
    fn test_get_template_definition() {
        let humanoid = get_template_definition("humanoid").unwrap();
        assert_eq!(humanoid.id, "humanoid");
        assert!(humanoid.is_builtin);
        assert!(humanoid.template.contains("${description}"));
        assert!(humanoid.template.contains("T-pose"));

        // Non-existent template
        assert!(get_template_definition("nonexistent").is_none());
    }

    #[test]
    fn test_builtin_template_cannot_be_deleted() {
        let result = delete_custom_template("humanoid");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Cannot delete builtin"));
    }
}
