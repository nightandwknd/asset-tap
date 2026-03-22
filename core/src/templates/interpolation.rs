//! Variable interpolation for template strings.

use anyhow::Result;
use std::collections::HashMap;

/// Interpolate variables into a template string.
///
/// Uses `${variable}` syntax for variable placeholders.
/// Replaces all occurrences of placeholders with their corresponding values.
///
/// # Arguments
///
/// * `template` - The template string with placeholders
/// * `variables` - Map of variable names to values
///
/// # Example
///
/// ```
/// use std::collections::HashMap;
/// use asset_tap_core::templates::interpolation::interpolate;
///
/// let mut vars = HashMap::new();
/// vars.insert("description".to_string(), "a cowboy ninja".to_string());
///
/// let result = interpolate("Test: ${description}", &vars).unwrap();
/// assert_eq!(result, "Test: a cowboy ninja");
/// ```
pub fn interpolate(template: &str, variables: &HashMap<String, String>) -> Result<String> {
    let mut result = template.to_string();

    // Replace ${var} placeholders
    for (key, value) in variables {
        let placeholder = format!("${{{}}}", key);
        result = result.replace(&placeholder, value);
    }

    // Warn about unresolved variables
    if result.contains("${") {
        tracing::warn!("Template contains unresolved variables: {}", template);
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interpolate_basic() {
        let mut vars = HashMap::new();
        vars.insert("description".to_string(), "a cowboy ninja".to_string());

        let result = interpolate("A character: ${description}.", &vars).unwrap();
        assert_eq!(result, "A character: a cowboy ninja.");
    }

    #[test]
    fn test_interpolate_multiple_variables() {
        let mut vars = HashMap::new();
        vars.insert("type".to_string(), "character".to_string());
        vars.insert("description".to_string(), "cowboy ninja".to_string());

        let result = interpolate("A ${type}: ${description}.", &vars).unwrap();
        assert_eq!(result, "A character: cowboy ninja.");
    }

    #[test]
    fn test_interpolate_no_variables() {
        let vars = HashMap::new();

        let result = interpolate("A simple template.", &vars).unwrap();
        assert_eq!(result, "A simple template.");
    }
}
