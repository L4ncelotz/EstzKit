//! Dependency resolution: topological sort of the selected packages plus
//! their transitive `depends_on` closure.

use std::collections::{HashMap, HashSet};

use crate::error::{CoreError, CoreResult};
use crate::manifest::PackageManifest;

/// Resolve `selected` package names to an ordered list of manifests where
/// every package appears after all of its dependencies.
///
/// Errors with [`CoreError::MissingDependency`] for unknown names and
/// [`CoreError::Cycle`] for dependency cycles.
pub fn resolve(
    selected: &[String],
    by_name: &HashMap<String, PackageManifest>,
) -> CoreResult<Vec<PackageManifest>> {
    let mut ordered = Vec::new();
    let mut done: HashSet<String> = HashSet::new();
    let mut visiting: Vec<String> = Vec::new();

    for name in selected {
        visit(name, by_name, &mut done, &mut visiting, &mut ordered)?;
    }
    Ok(ordered)
}

/// Depth-first visit: emit dependencies before the package itself.
fn visit(
    name: &str,
    by_name: &HashMap<String, PackageManifest>,
    done: &mut HashSet<String>,
    visiting: &mut Vec<String>,
    ordered: &mut Vec<PackageManifest>,
) -> CoreResult<()> {
    if done.contains(name) {
        return Ok(());
    }
    if let Some(pos) = visiting.iter().position(|n| n == name) {
        let mut cycle = visiting[pos..].to_vec();
        cycle.push(name.to_string());
        return Err(CoreError::Cycle(cycle.join(" -> ")));
    }
    let manifest = by_name
        .get(name)
        .ok_or_else(|| CoreError::MissingDependency(name.to_string()))?;
    visiting.push(name.to_string());
    for dep in &manifest.depends_on {
        visit(dep, by_name, done, visiting, ordered)?;
    }
    visiting.pop();
    done.insert(name.to_string());
    ordered.push(manifest.clone());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(name: &str, deps: &[&str]) -> PackageManifest {
        serde_yaml::from_str(&format!(
            "schema_version: 1\nname: {name}\ncategory: runtime\n\
             install: {{strategy: package_manager, scope: user}}\n\
             verify: {{command: echo, args: [], expect_regex: null}}\n\
             security: {{allow_scripts: false, allow_elevation: false}}\n\
             depends_on: [{}]\n",
            deps.join(", ")
        ))
        .expect("test manifest")
    }

    fn index(names: &[&str]) -> HashMap<String, PackageManifest> {
        names
            .iter()
            .map(|n| {
                let m = manifest(n, &[]);
                (n.to_string(), m)
            })
            .collect()
    }

    #[test]
    fn orders_dependencies_first() {
        let mut by_name = index(&["app", "lib"]);
        by_name.get_mut("app").unwrap().depends_on = vec!["lib".to_string()];
        let ordered = resolve(&["app".to_string()], &by_name).expect("resolves");
        assert_eq!(
            ordered.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
            vec!["lib", "app"]
        );
    }

    #[test]
    fn transitive_closure_included_once() {
        let mut by_name = index(&["a", "b", "c"]);
        by_name.get_mut("a").unwrap().depends_on = vec!["b".to_string()];
        by_name.get_mut("b").unwrap().depends_on = vec!["c".to_string()];
        let ordered = resolve(&["a".to_string(), "c".to_string()], &by_name).expect("resolves");
        let names: Vec<&str> = ordered.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["c", "b", "a"]);
        assert_eq!(names.iter().filter(|n| **n == "c").count(), 1);
    }

    #[test]
    fn detects_cycle() {
        let mut by_name = index(&["a", "b"]);
        by_name.get_mut("a").unwrap().depends_on = vec!["b".to_string()];
        by_name.get_mut("b").unwrap().depends_on = vec!["a".to_string()];
        let err = resolve(&["a".to_string()], &by_name).expect_err("cycle must fail");
        assert!(matches!(err, CoreError::Cycle(_)));
    }

    #[test]
    fn detects_missing_dependency() {
        let by_name = index(&["a"]);
        let err = resolve(&["a".to_string(), "ghost".to_string()], &by_name)
            .expect_err("missing dep must fail");
        assert!(matches!(err, CoreError::MissingDependency(_)));
    }
}
