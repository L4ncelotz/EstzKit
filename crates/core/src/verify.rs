//! Post-install verification: run the manifest verify command and match
//! its combined output against `expect_regex`.

use std::process::Command;

use regex::Regex;

use crate::error::{CoreError, CoreResult};
use crate::manifest::VerifySpec;

/// Run a command with args, returning combined stdout + stderr.
pub fn run_combined(command: &str, args: &[String]) -> CoreResult<String> {
    let output = Command::new(command)
        .args(args)
        .output()
        .map_err(|e| CoreError::ProcessSpawn(format!("{command}: {e}")))?;
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok(combined)
}

/// Check whether `output` matches `pattern`. A `None` pattern matches any
/// output (the caller relies on exit status only).
pub fn output_matches(pattern: Option<&str>, output: &str) -> CoreResult<bool> {
    match pattern {
        None => Ok(true),
        Some(p) => {
            let re = Regex::new(p).map_err(|e| CoreError::ManifestParse(e.to_string()))?;
            Ok(re.is_match(output))
        }
    }
}

/// Run the manifest verify command; `Ok(true)` means the package verifies.
pub fn run_verify(spec: &VerifySpec) -> CoreResult<bool> {
    let output = run_combined(&spec.command, &spec.args)?;
    output_matches(spec.expect_regex.as_deref(), &output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regex_matching() {
        assert!(output_matches(None, "anything").unwrap());
        assert!(output_matches(Some(r"v\d+\.\d+"), "node v20.11.0").unwrap());
        assert!(!output_matches(Some(r"v\d+"), "no digits here").unwrap());
    }

    #[test]
    fn invalid_regex_errors() {
        assert!(output_matches(Some("(["), "x").is_err());
    }
}
