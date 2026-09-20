//! Conditional parameters: `requires` / `conflicts_with`.
//!
//! Some provider parameters only apply in combination with another. Meshy
//! ignores `decimation_mode` unless `should_remesh` is on, ignores
//! `target_polycount` once `decimation_mode` is set ("When set,
//! `target_polycount` is ignored"), and outright *rejects* `aspect_ratio`
//! alongside `generate_multi_view`. A flat parameter list can't say any of
//! that, so the YAML declares the relationship and this module enforces it in
//! one place, for CLI, GUI and MCP alike.
//!
//! Two outcomes, and which one you get depends on whether *you* asked for the
//! value:
//!
//! - **You set it explicitly** (CLI `--param`, a GUI widget you touched, MCP
//!   `params`) and the condition doesn't hold → a [`ConditionViolation`]. The
//!   caller turns that into a usage error rather than quietly ignoring what
//!   you typed.
//! - **It's just the YAML default** → the key is [`Dropped`] from the request
//!   body, exactly as a null value is, so the provider applies its own
//!   default instead of receiving a field it would reject.

use super::config::ParameterDef;
use std::collections::{HashMap, HashSet};

/// A parameter removed from the request because its condition wasn't met.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped {
    /// The parameter that was dropped.
    pub param: String,
    /// Human-readable reason, e.g. `requires should_remesh=true (currently false)`.
    pub because: String,
}

/// An explicitly-set parameter whose condition isn't met.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConditionViolation {
    /// The parameter the user set.
    pub param: String,
    /// Human-readable explanation, e.g.
    /// `requires auto_size=true (currently false)`.
    pub detail: String,
}

impl std::fmt::Display for ConditionViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.param, self.detail)
    }
}

/// Does `actual` match a condition's declared `expected` value?
///
/// A declared `null` means "any non-null value" — the sibling just has to be
/// set to something, which is how you express "only when the user picked a
/// decimation mode at all".
fn matches(expected: &serde_json::Value, actual: Option<&serde_json::Value>) -> bool {
    let actual = actual.unwrap_or(&serde_json::Value::Null);
    if expected.is_null() {
        !actual.is_null()
    } else {
        expected == actual
    }
}

fn show(v: Option<&serde_json::Value>) -> String {
    match v {
        None | Some(serde_json::Value::Null) => "unset".to_string(),
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

fn show_expected(expected: &serde_json::Value) -> String {
    if expected.is_null() {
        "set".to_string()
    } else {
        show(Some(expected))
    }
}

/// Evaluate every parameter's `requires` / `conflicts_with` against the
/// effective values (YAML defaults with user overrides layered on).
///
/// A parameter is *active* when its effective value is non-null; an inactive
/// one is already absent from the request, so it neither fires a conflict nor
/// needs its own condition checked.
///
/// Returns the drops to apply (in declaration order, so logs and tests are
/// deterministic), or the first violation for a parameter the user set
/// explicitly. Evaluation runs to a fixed point, so a chain resolves: if `A`
/// requires `B` and `B` is itself dropped, `A` is dropped too.
pub fn evaluate_conditions(
    defs: &[ParameterDef],
    effective: &HashMap<String, serde_json::Value>,
    explicit: &HashSet<String>,
) -> Result<Vec<Dropped>, ConditionViolation> {
    let value_of = |live: &HashSet<&str>, name: &str| -> Option<serde_json::Value> {
        if !live.contains(name) {
            return None;
        }
        effective.get(name).filter(|v| !v.is_null()).cloned()
    };

    // Names still considered "set" this round. Dropping one removes it here,
    // which can in turn unmet a dependent's `requires` on the next pass.
    let mut live: HashSet<&str> = defs
        .iter()
        .filter(|d| {
            effective
                .get(&d.name)
                .map(|v| !v.is_null())
                .unwrap_or(false)
        })
        .map(|d| d.name.as_str())
        .collect();

    let mut dropped: Vec<Dropped> = Vec::new();

    loop {
        let mut progressed = false;

        for def in defs {
            if !live.contains(def.name.as_str()) {
                continue;
            }

            // `requires`: every listed sibling must hold its value.
            let unmet = def
                .requires
                .iter()
                .find(|(sibling, expected)| !matches(expected, value_of(&live, sibling).as_ref()));
            // `conflicts_with`: any listed sibling holding its value is fatal.
            // Also evaluated from the other side, so declaring the conflict on
            // one parameter is enough — the sibling that *fires* it gets
            // dropped when it is the default and the declaring one is explicit.
            let conflict = def
                .conflicts_with
                .iter()
                .find(|(sibling, expected)| matches(expected, value_of(&live, sibling).as_ref()))
                .map(|(s, e)| (s.clone(), e.clone()))
                .or_else(|| {
                    // Symmetric half: some other live parameter declares a
                    // conflict that this one's current value triggers.
                    defs.iter()
                        .filter(|other| {
                            other.name != def.name && live.contains(other.name.as_str())
                        })
                        .find_map(|other| {
                            other.conflicts_with.get(&def.name).and_then(|expected| {
                                matches(expected, value_of(&live, &def.name).as_ref())
                                    .then(|| (other.name.clone(), serde_json::Value::Null))
                            })
                        })
                });

            let (detail, is_conflict) = match (unmet, &conflict) {
                (Some((sibling, expected)), _) => (
                    format!(
                        "requires {sibling}={} (currently {})",
                        show_expected(expected),
                        show(value_of(&live, sibling).as_ref())
                    ),
                    false,
                ),
                (None, Some((sibling, _))) => (
                    format!(
                        "conflicts with {sibling}={}; clear one",
                        show(value_of(&live, sibling).as_ref())
                    ),
                    true,
                ),
                (None, None) => continue,
            };

            // The user asked for this value. Refuse rather than silently
            // discard it — except when the *other* side of a conflict is the
            // default, in which case dropping that side is the fix and this
            // one is left alone.
            if explicit.contains(&def.name) {
                if is_conflict {
                    let (sibling, _) = conflict.as_ref().expect("conflict present");
                    if !explicit.contains(sibling) {
                        // Default sibling loses; it gets dropped on its own
                        // turn through the loop (it sees this explicit one).
                        continue;
                    }
                }
                return Err(ConditionViolation {
                    param: def.name.clone(),
                    detail,
                });
            }

            live.remove(def.name.as_str());
            dropped.push(Dropped {
                param: def.name.clone(),
                because: detail,
            });
            progressed = true;
        }

        if !progressed {
            break;
        }
    }

    Ok(dropped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::config::ParameterType;

    fn def(name: &str, default: serde_json::Value) -> ParameterDef {
        ParameterDef {
            name: name.into(),
            label: name.into(),
            description: None,
            param_type: ParameterType::String,
            default,
            min: None,
            max: None,
            step: None,
            options: None,
            widget: None,
            allow_unset: false,
            requires: Default::default(),
            conflicts_with: Default::default(),
        }
    }

    fn values(pairs: &[(&str, serde_json::Value)]) -> HashMap<String, serde_json::Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    fn explicit(names: &[&str]) -> HashSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    /// `origin_at` only means something with `auto_size` on; left at its
    /// default it is dropped rather than sent into a request Meshy ignores.
    #[test]
    fn requires_unmet_drops_a_default_value() {
        let mut origin = def("origin_at", serde_json::json!("bottom"));
        origin
            .requires
            .insert("auto_size".into(), serde_json::json!(true));
        let defs = vec![def("auto_size", serde_json::json!(false)), origin];

        let dropped = evaluate_conditions(
            &defs,
            &values(&[
                ("auto_size", serde_json::json!(false)),
                ("origin_at", serde_json::json!("bottom")),
            ]),
            &explicit(&[]),
        )
        .expect("default drop, not a violation");

        assert_eq!(dropped.len(), 1);
        assert_eq!(dropped[0].param, "origin_at");
        assert!(dropped[0].because.contains("requires auto_size=true"));
    }

    #[test]
    fn requires_met_keeps_the_value() {
        let mut origin = def("origin_at", serde_json::json!("bottom"));
        origin
            .requires
            .insert("auto_size".into(), serde_json::json!(true));
        let defs = vec![def("auto_size", serde_json::json!(false)), origin];

        let dropped = evaluate_conditions(
            &defs,
            &values(&[
                ("auto_size", serde_json::json!(true)),
                ("origin_at", serde_json::json!("center")),
            ]),
            &explicit(&["auto_size", "origin_at"]),
        )
        .unwrap();
        assert!(dropped.is_empty());
    }

    /// Typing `--param origin_at=center` without `auto_size` is a mistake
    /// worth reporting, not something to silently discard.
    #[test]
    fn requires_unmet_on_an_explicit_value_is_a_violation() {
        let mut origin = def("origin_at", serde_json::json!("bottom"));
        origin
            .requires
            .insert("auto_size".into(), serde_json::json!(true));
        let defs = vec![def("auto_size", serde_json::json!(false)), origin];

        let err = evaluate_conditions(
            &defs,
            &values(&[
                ("auto_size", serde_json::json!(false)),
                ("origin_at", serde_json::json!("center")),
            ]),
            &explicit(&["origin_at"]),
        )
        .unwrap_err();
        assert_eq!(err.param, "origin_at");
        assert!(err.detail.contains("currently false"));
    }

    /// A `null` condition value means "set to anything".
    #[test]
    fn null_condition_means_any_non_null() {
        let mut poly = def("target_polycount", serde_json::json!(30000));
        poly.conflicts_with
            .insert("decimation_mode".into(), serde_json::Value::Null);
        let defs = vec![def("decimation_mode", serde_json::Value::Null), poly];

        // decimation_mode unset: no conflict.
        let dropped = evaluate_conditions(
            &defs,
            &values(&[
                ("decimation_mode", serde_json::Value::Null),
                ("target_polycount", serde_json::json!(30000)),
            ]),
            &explicit(&[]),
        )
        .unwrap();
        assert!(dropped.is_empty());

        // decimation_mode set: the default polycount is dropped, since Meshy
        // documents "When set, target_polycount is ignored".
        let dropped = evaluate_conditions(
            &defs,
            &values(&[
                ("decimation_mode", serde_json::json!(2)),
                ("target_polycount", serde_json::json!(30000)),
            ]),
            &explicit(&["decimation_mode"]),
        )
        .unwrap();
        assert_eq!(dropped.len(), 1);
        assert_eq!(dropped[0].param, "target_polycount");
    }

    /// The conflict is declared on `aspect_ratio` only, but turning on
    /// `generate_multi_view` has to drop the default aspect ratio all the same.
    #[test]
    fn conflict_declared_on_one_side_drops_the_defaulted_other() {
        let mut aspect = def("aspect_ratio", serde_json::json!("1:1"));
        aspect
            .conflicts_with
            .insert("generate_multi_view".into(), serde_json::json!(true));
        let defs = vec![aspect, def("generate_multi_view", serde_json::json!(false))];

        let dropped = evaluate_conditions(
            &defs,
            &values(&[
                ("aspect_ratio", serde_json::json!("1:1")),
                ("generate_multi_view", serde_json::json!(true)),
            ]),
            &explicit(&["generate_multi_view"]),
        )
        .unwrap();
        assert_eq!(dropped.len(), 1);
        assert_eq!(dropped[0].param, "aspect_ratio");
    }

    /// Setting the sibling explicitly while the declaring parameter is at its
    /// default drops the default — even though the conflict is declared the
    /// other way round.
    #[test]
    fn conflict_from_the_undeclared_side_still_resolves() {
        let mut aspect = def("aspect_ratio", serde_json::json!("1:1"));
        aspect
            .conflicts_with
            .insert("generate_multi_view".into(), serde_json::json!(true));
        // Declaration order reversed: multi-view is evaluated first.
        let defs = vec![def("generate_multi_view", serde_json::json!(false)), aspect];

        let dropped = evaluate_conditions(
            &defs,
            &values(&[
                ("generate_multi_view", serde_json::json!(true)),
                ("aspect_ratio", serde_json::json!("1:1")),
            ]),
            &explicit(&["generate_multi_view"]),
        )
        .unwrap();
        assert_eq!(dropped.len(), 1);
        assert_eq!(dropped[0].param, "aspect_ratio");
    }

    /// Asking for both halves of a mutually exclusive pair is a usage error:
    /// we can't guess which one you meant.
    #[test]
    fn both_sides_explicit_is_a_violation() {
        let mut aspect = def("aspect_ratio", serde_json::json!("1:1"));
        aspect
            .conflicts_with
            .insert("generate_multi_view".into(), serde_json::json!(true));
        let defs = vec![aspect, def("generate_multi_view", serde_json::json!(false))];

        let err = evaluate_conditions(
            &defs,
            &values(&[
                ("aspect_ratio", serde_json::json!("16:9")),
                ("generate_multi_view", serde_json::json!(true)),
            ]),
            &explicit(&["aspect_ratio", "generate_multi_view"]),
        )
        .unwrap_err();
        assert_eq!(err.param, "aspect_ratio");
        assert!(err.detail.contains("conflicts with generate_multi_view"));
    }

    /// A dropped parameter can't satisfy someone else's `requires`.
    #[test]
    fn chained_conditions_drop_together() {
        let mut b = def("b", serde_json::json!("on"));
        b.requires.insert("a".into(), serde_json::json!(true));
        let mut c = def("c", serde_json::json!("on"));
        c.requires.insert("b".into(), serde_json::json!("on"));
        let defs = vec![def("a", serde_json::json!(false)), b, c];

        let dropped = evaluate_conditions(
            &defs,
            &values(&[
                ("a", serde_json::json!(false)),
                ("b", serde_json::json!("on")),
                ("c", serde_json::json!("on")),
            ]),
            &explicit(&[]),
        )
        .unwrap();
        let names: Vec<&str> = dropped.iter().map(|d| d.param.as_str()).collect();
        assert_eq!(names, vec!["b", "c"]);
    }

    #[test]
    fn no_conditions_is_a_no_op() {
        let defs = vec![
            def("a", serde_json::json!(1)),
            def("b", serde_json::json!(2)),
        ];
        let dropped = evaluate_conditions(
            &defs,
            &values(&[("a", serde_json::json!(1)), ("b", serde_json::json!(2))]),
            &explicit(&["a"]),
        )
        .unwrap();
        assert!(dropped.is_empty());
    }
}
