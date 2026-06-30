//! `typio-vet` — load a native Typio C ABI engine artifact and vet it across
//! ABI, behavior, and packaged resources.

use std::ffi::{c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use typio_vet::{
    resource, scenario, CheckCategory, CheckResult, CheckStatus, Summary, TypioEngineInfo,
    TypioEngineType, TypioKeyboardEngine, TypioVoiceEngine,
};

const USAGE: &str = "\
Usage: typio-vet <engine-abi.so> [options]

Vet a native Typio C ABI engine artifact: ABI surface, runtime behavior, and
packaged resources. Exits non-zero only if a check FAILs (warnings do not
block). This CLI loads the artifact inside the vet process; it does not vet
manifest-declared engine worker executables.

Options:
    --package <dir>    Package root for resource checks (auto-detected otherwise)
    --only <dims>      Comma-separated dimensions: abi, behavior, resource
    --check <name>     Run/report only the named check
    --list             List the dimensions and exit
    --help, -h         Show this message

Examples:
    typio-vet ../typio-engine-basic/target/debug/libtypio_engine_basic.so
    typio-vet ./libtypio_engine_rime.so --only abi,resource
    typio-vet ./libtypio_engine_whisper.so --package ../typio-engine-whisper";

struct Args {
    artifact_path: String,
    package: Option<PathBuf>,
    only: Option<Vec<CheckCategory>>,
    check: Option<String>,
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();

    if argv.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    if argv.iter().any(|a| a == "--list") {
        println!("Vetting dimensions:");
        println!("  abi       TypioEngineInfo, struct sizes, vtable completeness");
        println!("  behavior  invariants observed by driving the engine");
        println!("  resource  packaged assets (freedesktop icons)");
        return ExitCode::SUCCESS;
    }

    let args = match parse_args(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}\n");
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };

    match run(&args) {
        Ok(summary) if summary.is_failure() => ExitCode::FAILURE,
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut artifact_path = None;
    let mut package = None;
    let mut only = None;
    let mut check = None;

    let mut i = 1;
    while i < argv.len() {
        match argv[i].as_str() {
            "--package" => {
                i += 1;
                package = Some(PathBuf::from(
                    argv.get(i).ok_or("--package needs a directory")?,
                ));
            }
            "--only" => {
                i += 1;
                let spec = argv.get(i).ok_or("--only needs a value")?;
                let mut cats = Vec::new();
                for part in spec.split(',') {
                    cats.push(match part.trim() {
                        "abi" => CheckCategory::Abi,
                        "behavior" => CheckCategory::Behavior,
                        "resource" => CheckCategory::Resource,
                        other => return Err(format!("unknown dimension '{other}'")),
                    });
                }
                only = Some(cats);
            }
            "--check" => {
                i += 1;
                check = Some(argv.get(i).ok_or("--check needs a name")?.clone());
            }
            other if other.starts_with('-') => return Err(format!("unknown option '{other}'")),
            other => {
                if artifact_path.is_some() {
                    return Err(format!("unexpected argument '{other}'"));
                }
                artifact_path = Some(other.to_string());
            }
        }
        i += 1;
    }

    Ok(Args {
        artifact_path: artifact_path.ok_or("missing <engine-abi.so>")?,
        package,
        only,
        check,
    })
}

fn run(args: &Args) -> Result<Summary, String> {
    let artifact_path = Path::new(&args.artifact_path);
    let pkg = args
        .package
        .clone()
        .or_else(|| resource::discover_package(artifact_path));

    unsafe {
        let c_path = CString::new(args.artifact_path.as_str()).map_err(|_| "path contains NUL")?;
        let handle = libc::dlopen(c_path.as_ptr(), libc::RTLD_NOW | libc::RTLD_GLOBAL);
        if handle.is_null() {
            let err = CStr::from_ptr(libc::dlerror()).to_string_lossy();
            return Err(format!("failed to load {}: {err}", args.artifact_path));
        }

        let get_info = dlsym::<unsafe extern "C" fn() -> *const TypioEngineInfo>(
            handle,
            b"typio_engine_get_info\0",
        )
        .ok_or("missing export 'typio_engine_get_info'")?;

        let info = get_info();
        if info.is_null() || (*info).name.is_null() {
            return Err("TypioEngineInfo is null or malformed".to_string());
        }
        let name = CStr::from_ptr((*info).name).to_string_lossy();
        let kind = (*info).type_;

        println!(
            "typio-vet: {} (name={name}, type={kind:?})",
            args.artifact_path
        );
        if let Some(p) = &pkg {
            println!("           package: {}", p.display());
        } else {
            println!("           package: <not found> (resource asset checks limited)");
        }

        let mut results = match kind {
            TypioEngineType::TypioEngineTypeKeyboard => {
                let create = dlsym::<unsafe extern "C" fn() -> *mut TypioKeyboardEngine>(
                    handle,
                    b"typio_keyboard_engine_create\0",
                )
                .ok_or("missing export 'typio_keyboard_engine_create'")?;
                let mut r = scenario::keyboard_checks(create);
                r.extend(resource::resource_checks(info, pkg.as_deref()));
                r
            }
            TypioEngineType::TypioEngineTypeVoice => {
                let create = dlsym::<unsafe extern "C" fn() -> *mut TypioVoiceEngine>(
                    handle,
                    b"typio_voice_engine_create\0",
                )
                .ok_or("missing export 'typio_voice_engine_create'")?;
                let mut r = scenario::voice_checks(create);
                r.extend(resource::resource_checks(info, pkg.as_deref()));
                r
            }
            other => return Err(format!("unsupported engine type {other:?}")),
        };

        if let Some(cats) = &args.only {
            results.retain(|r| cats.contains(&r.category));
        }
        if let Some(name) = &args.check {
            results.retain(|r| r.name == name);
            if results.is_empty() {
                return Err(format!("no check named '{name}'"));
            }
        }

        let summary = Summary::of(&results);
        report(&results, summary);
        Ok(summary)
    }
}

unsafe fn dlsym<T>(handle: *mut c_void, name: &[u8]) -> Option<T> {
    let sym = libc::dlsym(handle, name.as_ptr() as *const i8);
    if sym.is_null() {
        return None;
    }
    Some(std::mem::transmute_copy(&sym))
}

fn report(results: &[CheckResult], summary: Summary) {
    let width = results
        .iter()
        .map(|r| r.name.len())
        .max()
        .unwrap_or(20)
        .max(12);

    for cat in [
        CheckCategory::Abi,
        CheckCategory::Behavior,
        CheckCategory::Resource,
    ] {
        let group: Vec<&CheckResult> = results.iter().filter(|r| r.category == cat).collect();
        if group.is_empty() {
            continue;
        }
        println!("\n  {}", cat.label());
        for r in group {
            let dots = ".".repeat((width + 4).saturating_sub(r.name.len()));
            println!("    {} {} {}", r.name, dots, marker(r.status));
            if !r.detail.is_empty() {
                println!("        -> {}", r.detail);
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
