//! `estzkit-cli`: a thin command-line wrapper over `estzkit-core`.
//!
//! Subcommands: `detect`, `profiles`, `packages`, `plan <names...>`,
//! `install <names...>`. Manifests come from `--manifest-dir`, then
//! `$ESTZKIT_MANIFESTS`, then `./manifests`.

use std::collections::HashSet;
use std::path::PathBuf;

use estzkit_core::manifest::{DirSource, ManifestSource};
use estzkit_core::provider::{provider_for, WingetProvider};
use estzkit_core::{
    build_plan, detect_package, resolve, resume_plan, run_plan, CoreError, CoreResult, EventSink,
    PackageManifest, ProviderKind, StepEvent, Store,
};

/// Command-line sink that prints each [`StepEvent`] to stdout.
struct StdoutSink;

impl EventSink for StdoutSink {
    fn emit(&mut self, event: StepEvent) {
        let detail = event.message.as_deref().or(event.error.as_deref()).unwrap_or("");
        println!(
            "[{}] {} {}",
            event.plan_id,
            event.package_id,
            event.state.as_str()
        );
        if !detail.is_empty() {
            println!("    {detail}");
        }
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("estzkit-cli: {e}");
        std::process::exit(1);
    }
}

fn run() -> CoreResult<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (manifest_dir, command) = parse_args(&args);

    let dir = DirSource::resolve_dir(manifest_dir.as_deref());
    let source = DirSource::new(dir);
    let packages = source.packages()?;
    match command.as_str() {
        "packages" => print_packages(&packages),
        "profiles" => print_profiles(&source)?,
        "detect" => detect_all(&packages),
        "plan" => {
            let names: Vec<String> = args
                .iter()
                .skip_while(|a| *a != "plan")
                .skip(1)
                .filter(|a| !a.starts_with("--"))
                .cloned()
                .collect();
            print_plan(&packages, &names)?;
        }
        "install" => {
            let names: Vec<String> = args
                .iter()
                .skip_while(|a| *a != "install")
                .skip(1)
                .filter(|a| !a.starts_with("--"))
                .cloned()
                .collect();
            install(&packages, &names)?;
        }
        "resume" => {
            let plan_id = args
                .iter()
                .skip_while(|a| *a != "resume")
                .nth(1)
                .ok_or_else(|| CoreError::PlanNotFound("missing plan_id argument".to_string()))?;
            resume(&packages, plan_id)?;
        }
        other => {
            eprintln!("unknown command: {other}");
            print_usage();
            std::process::exit(2);
        }
    }
    Ok(())
}

fn parse_args(args: &[String]) -> (Option<PathBuf>, String) {
    let mut manifest_dir: Option<PathBuf> = None;
    let mut command = String::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--manifest-dir" {
            i += 1;
            if i < args.len() {
                manifest_dir = Some(PathBuf::from(&args[i]));
            }
        } else if let Some(rest) = arg.strip_prefix("--manifest-dir=") {
            manifest_dir = Some(PathBuf::from(rest));
        } else if command.is_empty() {
            command = arg.clone();
        }
        i += 1;
    }
    (manifest_dir, command)
}

fn print_usage() {
    println!("estzkit-cli [--manifest-dir <dir>] <command>");
    println!("  packages            list available packages");
    println!("  profiles            list profiles");
    println!("  detect              detect which packages are installed");
    println!("  plan <names...>     preview an install plan");
    println!("  install <names...>  install the named packages");
    println!("  resume <plan_id>    resume a plan from store");
}

fn print_packages(packages: &[PackageManifest]) {
    println!("{:<20} {}", "PACKAGE", "CATEGORY");
    for p in packages {
        println!("{:<20} {}", p.name, p.category);
    }
    println!("{} packages", packages.len());
}

fn print_profiles(source: &DirSource) -> CoreResult<()> {
    let profiles = source.profiles()?;
    println!("{:<24} {}", "PROFILE", "PACKAGES");
    for profile in &profiles {
        println!(
            "{:<24} {}",
            profile.name,
            profile.packages.join(", ")
        );
    }
    println!("{} profiles", profiles.len());
    Ok(())
}

fn detect_all(packages: &[PackageManifest]) {
    let winget = WingetProvider::new();
    println!("{:<20} {:<10} {}", "PACKAGE", "FOUND", "VERSION");
    for manifest in packages {
        let result = detect_package(manifest, &winget);
        println!(
            "{:<20} {:<10} {}",
            result.package.name,
            result.package.found,
            result.package.version.as_deref().unwrap_or("-")
        );
    }
}

fn resolve_and_detect(
    packages: &[PackageManifest],
    names: &[String],
) -> CoreResult<(Vec<PackageManifest>, HashSet<String>)> {
    let by_name = estzkit_core::index_by_name(packages);
    let ordered = resolve(names, &by_name)?;
    let winget = WingetProvider::new();
    let mut already = HashSet::new();
    for manifest in &ordered {
        if detect_package(manifest, &winget).package.found {
            already.insert(manifest.name.clone());
        }
    }
    Ok((ordered, already))
}

fn print_plan(packages: &[PackageManifest], names: &[String]) -> CoreResult<()> {
    let (ordered, already) = resolve_and_detect(packages, names)?;
    let plan = build_plan(&ordered, &already);
    println!("plan {}", plan.id);
    println!("uac_warnings: {} (UAC prompts expected)", plan.uac_warnings);
    for step in &plan.steps {
        println!(
            "  {:<20} scope={} state={}",
            step.package_id,
            step.scope.as_str(),
            step.state.as_str()
        );
    }
    Ok(())
}

fn install(packages: &[PackageManifest], names: &[String]) -> CoreResult<()> {
    let (ordered, already) = resolve_and_detect(packages, names)?;
    let by_name = estzkit_core::index_by_name(packages);
    let plan = build_plan(&ordered, &already);
    println!("plan {}: {} steps, {} uac_warnings", plan.id, plan.steps.len(), plan.uac_warnings);

    let provider = provider_for(ProviderKind::Winget);
    let store_path = std::env::var("ESTZKIT_STORE").unwrap_or_else(|_| "estzkit.db".to_string());
    let mut store = Store::open(&store_path)?;
    let mut sink = StdoutSink;
    run_plan(&plan, &by_name, provider.as_ref(), &mut store, &mut sink)?;
    let steps = store.steps_for_plan(&plan.id)?;
    if steps.iter().any(|s| s.state != "Completed" && s.state != "Skipped") {
        return Err(CoreError::PlanFailed(plan.id));
    }
    Ok(())
}

fn resume(packages: &[PackageManifest], plan_id: &str) -> CoreResult<()> {
    let by_name = estzkit_core::index_by_name(packages);
    let provider = provider_for(ProviderKind::Winget);
    let store_path = std::env::var("ESTZKIT_STORE").unwrap_or_else(|_| "estzkit.db".to_string());
    let mut store = Store::open(&store_path)?;
    let mut sink = StdoutSink;
    println!("resuming plan {plan_id}");
    resume_plan(plan_id, &by_name, provider.as_ref(), &mut store, &mut sink)?;
    let steps = store.steps_for_plan(plan_id)?;
    if steps.iter().any(|s| s.state != "Completed" && s.state != "Skipped") {
        return Err(CoreError::PlanFailed(plan_id.to_string()));
    }
    Ok(())
}
