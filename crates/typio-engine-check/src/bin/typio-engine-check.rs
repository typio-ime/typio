//! Command-line black-box conformance gate for Typio engine processes.

use std::path::PathBuf;
use std::process::ExitCode;

use typio_engine_check::{
    CheckCategory, CheckResult, CheckStatus, Summary, VetReport, resource, vet_manifest,
};

const USAGE: &str = "\
Usage: typio-engine-check <typio-engine-*.toml> [options]

Start the manifest-declared engine in an isolated process and verify its
Engine Protocol handshake, runtime behavior, and packaged resources. Warnings
do not fail the gate.

Options:
    --package <dir>    Package root for resource checks (auto-detected otherwise)
    --only <dims>      Comma-separated: protocol, behavior, resource
    --check <name>     Run/report only the named check
    --list             List dimensions and exit
    --help, -h         Show this message

Examples:
    typio-engine-check ../typio-engine-compose/typio-engine-compose.toml
    typio-engine-check ./typio-engine-rime.toml --only protocol,resource";

struct Args {
    manifest_path: PathBuf,
    package: Option<PathBuf>,
    only: Option<Vec<CheckCategory>>,
    check: Option<String>,
}

fn main() -> ExitCode {
    let argv = std::env::args().collect::<Vec<_>>();
    if argv
        .iter()
        .any(|argument| argument == "--help" || argument == "-h")
    {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    if argv.iter().any(|argument| argument == "--list") {
        println!("Vetting dimensions:");
        println!("  protocol  manifest, frames, handshake, identity, schema");
        println!("  behavior  lifecycle and modality operations over IPC");
        println!("  resource  packaged assets such as freedesktop icons");
        return ExitCode::SUCCESS;
    }

    let args = match parse_args(&argv) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("error: {error}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    match run(args) {
        Ok(summary) if summary.is_failure() => ExitCode::FAILURE,
        Ok(_) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut manifest_path = None;
    let mut package = None;
    let mut only = None;
    let mut check = None;
    let mut index = 1;
    while index < argv.len() {
        match argv[index].as_str() {
            "--package" => {
                index += 1;
                package = Some(PathBuf::from(
                    argv.get(index).ok_or("--package needs a directory")?,
                ));
            }
            "--only" => {
                index += 1;
                let value = argv.get(index).ok_or("--only needs a value")?;
                let mut categories = Vec::new();
                for part in value.split(',') {
                    categories.push(match part.trim() {
                        "protocol" => CheckCategory::Protocol,
                        "behavior" => CheckCategory::Behavior,
                        "resource" => CheckCategory::Resource,
                        other => return Err(format!("unknown dimension '{other}'")),
                    });
                }
                only = Some(categories);
            }
            "--check" => {
                index += 1;
                check = Some(argv.get(index).ok_or("--check needs a name")?.clone());
            }
            option if option.starts_with('-') => {
                return Err(format!("unknown option '{option}'"));
            }
            path => {
                if manifest_path.is_some() {
                    return Err(format!("unexpected argument '{path}'"));
                }
                manifest_path = Some(PathBuf::from(path));
            }
        }
        index += 1;
    }
    Ok(Args {
        manifest_path: manifest_path.ok_or("missing <typio-engine-*.toml>")?,
        package,
        only,
        check,
    })
}

fn run(args: Args) -> Result<Summary, String> {
    let mut report = vet_manifest(&args.manifest_path, args.package.as_deref())
        .map_err(|error| error.to_string())?;
    filter_results(&mut report, args.only.as_deref(), args.check.as_deref())?;

    let package = args
        .package
        .or_else(|| resource::discover_package(&args.manifest_path));
    println!(
        "typio-engine-check: {} (name={}, type={})",
        report.manifest_path.display(),
        report.manifest.name,
        report.manifest.engine_type
    );
    println!(
        "           package: {}",
        package
            .as_deref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<not found> (resource checks limited)".to_string())
    );

    let summary = report.summary();
    report_results(&report.results, summary);
    Ok(summary)
}

fn filter_results(
    report: &mut VetReport,
    only: Option<&[CheckCategory]>,
    check: Option<&str>,
) -> Result<(), String> {
    if let Some(categories) = only {
        report
            .results
            .retain(|result| categories.contains(&result.category));
    }
    if let Some(name) = check {
        report.results.retain(|result| result.name == name);
        if report.results.is_empty() {
            return Err(format!("no check named '{name}'"));
        }
    }
    Ok(())
}

fn report_results(results: &[CheckResult], summary: Summary) {
    let width = results
        .iter()
        .map(|result| result.name.len())
        .max()
        .unwrap_or(20)
        .max(12);
    for category in [
        CheckCategory::Protocol,
        CheckCategory::Behavior,
        CheckCategory::Resource,
    ] {
        let group = results
            .iter()
            .filter(|result| result.category == category)
            .collect::<Vec<_>>();
        if group.is_empty() {
            continue;
        }
        println!("\n  {}", category.label());
        for result in group {
            let dots = ".".repeat((width + 4).saturating_sub(result.name.len()));
            println!("    {} {} {}", result.name, dots, marker(result.status));
            if !result.detail.is_empty() {
                println!("        -> {}", result.detail);
            }
        }
    }
    println!(
        "\n{} passed, {} warnings, {} failed",
        summary.passed, summary.warned, summary.failed
    );
}

fn marker(status: CheckStatus) -> &'static str {
    status.label()
}
