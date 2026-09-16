//! Command handlers for the resource+verb CLI (ADR-0004).
//!
//! Every command maps to one or more TIP v1 RPCs (see
//! `docs/reference/cli.md` for the full table). Output is rendered as
//! either human-readable text ("plain") or raw JSON depending on the
//! global `--output` flag.

use std::io::{self, Write};

use serde_json::{Value, json};
use typio_client::Client;

/// Output format chosen by the global `--output`/`-o` flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Plain,
    Json,
}

fn connect() -> io::Result<Client> {
    Client::connect()
}

fn print_json(value: &Value) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer_pretty(&mut stdout, value).map_err(io::Error::other)?;
    writeln!(stdout)
}

fn engine_key(name: &str, key: &str) -> String {
    format!("engines.{name}.{key}")
}

/* ---------------------------------------------------------------- */
/* engine.*                                                         */
/* ---------------------------------------------------------------- */

pub fn engine_list(out: OutputFormat) -> io::Result<()> {
    let mut c = connect()?;
    let result = c.call("engine.list", json!({}))?;
    if out == OutputFormat::Json {
        return print_json(&result);
    }
    let Some(arr) = result.as_array() else {
        return Ok(());
    };
    for entry in arr {
        let name = entry.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let kind = entry.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        let display = entry
            .get("displayName")
            .and_then(|v| v.as_str())
            .unwrap_or(name);
        let active = entry
            .get("active")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let marker = if active { "*" } else { " " };
        println!("{marker} {name:16} {kind:8} {display}");
    }
    Ok(())
}

pub fn engine_show(name: &str, out: OutputFormat) -> io::Result<()> {
    let mut c = connect()?;
    let result = c.call("engine.describe", json!({ "name": name }))?;
    if out == OutputFormat::Json {
        return print_json(&result);
    }
    let display = result
        .get("displayName")
        .and_then(|v| v.as_str())
        .unwrap_or(name);
    let kind = result.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    println!("{display} ({name}, {kind})");

    let props = result.get("properties").and_then(|v| v.as_array());
    if let Some(props) = props
        && !props.is_empty()
    {
        println!("\nproperties:");
        for p in props {
            let key = p.get("key").and_then(|v| v.as_str()).unwrap_or("");
            let ty = p.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let val = p.get("value").cloned().unwrap_or(Value::Null);
            let label = p.get("label").and_then(|v| v.as_str()).unwrap_or("");
            print!("  {key:32} = {val} ({ty})");
            if !label.is_empty() {
                print!("  -- {label}");
            }
            println!();
        }
    }

    let cmds = result.get("commands").and_then(|v| v.as_array());
    if let Some(cmds) = cmds
        && !cmds.is_empty()
    {
        println!("\ncommands:");
        for cmd in cmds {
            let id = cmd.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let label = cmd.get("label").and_then(|v| v.as_str()).unwrap_or("");
            println!("  {id:24}  {label}");
        }
    }
    Ok(())
}

pub fn engine_use(name: &str, out: OutputFormat) -> io::Result<()> {
    // Protocol v2 (ADR-0026) requires modality-explicit `keyboard.use` /
    // `voice.use`. The CLI stays kind-agnostic by resolving the engine's kind
    // from `engine.list` and dispatching to the matching verb.
    let mut c = connect()?;
    let list = c.call("engine.list", json!({}))?;
    let kind = list
        .as_array()
        .and_then(|arr| {
            arr.iter().find_map(|e| {
                let ename = e.get("name").and_then(|v| v.as_str())?;
                let ekind = e.get("kind").and_then(|v| v.as_str())?;
                (ename == name).then(|| ekind.to_string())
            })
        })
        .unwrap_or_else(|| "keyboard".to_string());
    let method = match kind.as_str() {
        "voice" => "voice.use",
        _ => "keyboard.use",
    };
    let result = c.call(method, json!({ "name": name }))?;
    if out == OutputFormat::Json {
        return print_json(&result);
    }
    Ok(())
}

pub fn engine_next(kind: Option<&str>, out: OutputFormat) -> io::Result<()> {
    // Protocol v2 (ADR-0026): `keyboard.next` / `voice.next`. Defaults to the
    // keyboard slot when no kind is given (preserves the pre-v2 default).
    let method = match kind {
        Some("voice") => "voice.next",
        _ => "keyboard.next",
    };
    let mut c = connect()?;
    let result = c.call(method, json!({}))?;
    if out == OutputFormat::Json {
        return print_json(&result);
    }
    if let Some(active) = result.get("active").and_then(|v| v.as_str()) {
        println!("{active}");
    }
    Ok(())
}

pub fn engine_props(name: &str, out: OutputFormat) -> io::Result<()> {
    let mut c = connect()?;
    let result = c.call("engine.describe", json!({ "name": name }))?;
    let props = result
        .get("properties")
        .cloned()
        .unwrap_or(Value::Array(vec![]));
    if out == OutputFormat::Json {
        return print_json(&props);
    }
    let arr = props.as_array().cloned().unwrap_or_default();
    for p in arr {
        let key = p.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let ty = p.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let val = p.get("value").cloned().unwrap_or(Value::Null);
        println!("{key:32} = {val} ({ty})");
    }
    Ok(())
}

pub fn engine_actions(name: &str, out: OutputFormat) -> io::Result<()> {
    let mut c = connect()?;
    let result = c.call("engine.describe", json!({ "name": name }))?;
    let cmds = result
        .get("commands")
        .cloned()
        .unwrap_or(Value::Array(vec![]));
    if out == OutputFormat::Json {
        return print_json(&cmds);
    }
    let arr = cmds.as_array().cloned().unwrap_or_default();
    for cmd in arr {
        let id = cmd.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let label = cmd.get("label").and_then(|v| v.as_str()).unwrap_or("");
        println!("{id:24}  {label}");
    }
    Ok(())
}

pub fn engine_get(name: &str, key: &str, out: OutputFormat) -> io::Result<()> {
    let mut c = connect()?;
    let result = c.call("config.get", json!({ "key": engine_key(name, key) }))?;
    if out == OutputFormat::Json {
        return print_json(&result);
    }
    if let Some(v) = result.get("value") {
        match v {
            Value::String(s) => println!("{s}"),
            other => println!("{other}"),
        }
    }
    Ok(())
}

pub fn engine_set(name: &str, key: &str, value: &str, out: OutputFormat) -> io::Result<()> {
    let mut c = connect()?;
    let result = c.call(
        "config.set",
        json!({ "key": engine_key(name, key), "value": value }),
    )?;
    if out == OutputFormat::Json {
        return print_json(&result);
    }
    Ok(())
}

pub fn engine_do(name: &str, command: &str, out: OutputFormat) -> io::Result<()> {
    let mut c = connect()?;
    let result = c.call("engine.invoke", json!({ "name": name, "command": command }))?;
    if out == OutputFormat::Json {
        return print_json(&result);
    }
    Ok(())
}

pub fn engine_setup(name: Option<&str>, out: OutputFormat) -> io::Result<()> {
    match name {
        Some(n) => {
            let mut c = connect()?;
            let result = c.call("engine.invoke", json!({ "name": n, "command": "setup" }))?;
            if out == OutputFormat::Json {
                return print_json(&result);
            }
            println!("Setup complete for {n}.");
            Ok(())
        }
        None => {
            let mut c = connect()?;
            let engines = c.call("engine.list", json!({}))?;
            let Some(arr) = engines.as_array() else {
                return Ok(());
            };
            let mut found = false;
            for entry in arr {
                let ename = entry.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let desc = c.call("engine.describe", json!({ "name": ename }))?;
                let cmds = desc.get("commands").and_then(|v| v.as_array());
                if let Some(cmds) = cmds {
                    for cmd in cmds {
                        let id = cmd.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        if id == "setup" {
                            let label = cmd.get("label").and_then(|v| v.as_str()).unwrap_or("");
                            let display = desc
                                .get("displayName")
                                .and_then(|v| v.as_str())
                                .unwrap_or(ename);
                            if out == OutputFormat::Json {
                                print_json(
                                    &json!({ "name": ename, "displayName": display, "setupLabel": label }),
                                )?;
                            } else {
                                println!("{ename:16} {display:24} {label}");
                            }
                            found = true;
                            break;
                        }
                    }
                }
            }
            if !found && out == OutputFormat::Plain {
                eprintln!("No engines with a setup command found.");
            }
            Ok(())
        }
    }
}

pub fn engine_load(path: &str, out: OutputFormat) -> io::Result<()> {
    let mut c = connect()?;
    let result = c.call("engine.load", json!({ "path": path }))?;
    if out == OutputFormat::Json {
        return print_json(&result);
    }
    let loaded = result
        .get("loaded")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let path = result.get("path").and_then(|v| v.as_str()).unwrap_or("");
    if loaded {
        println!("Loaded engine from {path}");
    }
    Ok(())
}

pub fn engine_unload(name: &str, out: OutputFormat) -> io::Result<()> {
    let mut c = connect()?;
    let result = c.call("engine.unload", json!({ "name": name }))?;
    if out == OutputFormat::Json {
        return print_json(&result);
    }
    let unloaded = result
        .get("unloaded")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let name = result.get("name").and_then(|v| v.as_str()).unwrap_or("");
    if unloaded {
        println!("Unloaded engine {name}");
    }
    Ok(())
}

pub fn engine_reload(name: &str, path: Option<&str>, out: OutputFormat) -> io::Result<()> {
    let mut c = connect()?;
    let params = match path {
        Some(p) => json!({ "name": name, "path": p }),
        None => json!({ "name": name }),
    };
    let result = c.call("engine.reload", params)?;
    if out == OutputFormat::Json {
        return print_json(&result);
    }
    let reloaded = result
        .get("reloaded")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let name = result.get("name").and_then(|v| v.as_str()).unwrap_or("");
    if reloaded {
        if let Some(p) = result.get("path").and_then(|v| v.as_str()) {
            println!("Reloaded engine {name} from {p}");
        } else {
            println!("Reloaded engine {name}");
        }
    }
    Ok(())
}

/* ---------------------------------------------------------------- */
/* language.*                                                       */
/* ---------------------------------------------------------------- */

pub fn language_list(out: OutputFormat) -> io::Result<()> {
    let mut c = connect()?;
    let result = c.call("language.list", json!({}))?;
    if out == OutputFormat::Json {
        return print_json(&result);
    }
    let Some(arr) = result.get("languages").and_then(|v| v.as_array()) else {
        return Ok(());
    };
    for entry in arr {
        let tag = entry.get("tag").and_then(|v| v.as_str()).unwrap_or("");
        let active = entry
            .get("active")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let marker = if active { "*" } else { " " };
        println!("{marker} {tag}");
    }
    Ok(())
}

pub fn language_use(tag: &str, out: OutputFormat) -> io::Result<()> {
    let mut c = connect()?;
    let result = c.call("language.use", json!({ "tag": tag }))?;
    if out == OutputFormat::Json {
        return print_json(&result);
    }
    Ok(())
}

pub fn language_cycle(forward: bool, out: OutputFormat) -> io::Result<()> {
    let mut c = connect()?;
    let method = if forward {
        "language.next"
    } else {
        "language.prev"
    };
    let result = c.call(method, json!({}))?;
    if out == OutputFormat::Json {
        return print_json(&result);
    }
    if let Some(active) = result.get("active").and_then(|v| v.as_str()) {
        println!("{active}");
    }
    Ok(())
}

/* ---------------------------------------------------------------- */
/* config.*                                                         */
/* ---------------------------------------------------------------- */

pub fn config_get(key: &str, out: OutputFormat) -> io::Result<()> {
    let mut c = connect()?;
    let result = c.call("config.get", json!({ "key": key }))?;
    if out == OutputFormat::Json {
        return print_json(&result);
    }
    if let Some(v) = result.get("value") {
        match v {
            Value::String(s) => println!("{s}"),
            other => println!("{other}"),
        }
    }
    Ok(())
}

pub fn config_set(key: &str, value: &str, out: OutputFormat) -> io::Result<()> {
    let mut c = connect()?;
    let result = c.call("config.set", json!({ "key": key, "value": value }))?;
    if out == OutputFormat::Json {
        return print_json(&result);
    }
    Ok(())
}

pub fn config_unset(key: &str, out: OutputFormat) -> io::Result<()> {
    let mut c = connect()?;
    let result = c.call("config.unset", json!({ "key": key }))?;
    if out == OutputFormat::Json {
        return print_json(&result);
    }
    Ok(())
}

pub fn config_list(prefix: Option<&str>, out: OutputFormat) -> io::Result<()> {
    let mut c = connect()?;
    let params = match prefix {
        Some(p) => json!({ "prefix": p }),
        None => json!({}),
    };
    let result = c.call("config.list", params)?;
    if out == OutputFormat::Json {
        return print_json(&result);
    }
    let Some(arr) = result.as_array() else {
        return Ok(());
    };
    for entry in arr {
        let key = entry.get("key").and_then(|v| v.as_str()).unwrap_or("");
        let ty = entry.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let val = entry.get("value").cloned().unwrap_or(Value::Null);
        println!("{key:40} = {val} ({ty})");
    }
    Ok(())
}

pub fn config_show(out: OutputFormat) -> io::Result<()> {
    let mut c = connect()?;
    let result = c.call("config.show", json!({}))?;
    if out == OutputFormat::Json {
        return print_json(&result);
    }
    if let Some(text) = result.get("text").and_then(|v| v.as_str()) {
        print!("{text}");
    }
    Ok(())
}

pub fn config_reload(out: OutputFormat) -> io::Result<()> {
    let mut c = connect()?;
    let result = c.call("config.reload", json!({}))?;
    if out == OutputFormat::Json {
        return print_json(&result);
    }
    Ok(())
}

pub fn config_edit(out: OutputFormat) -> io::Result<()> {
    use std::process::Command;

    let mut c = connect()?;
    let result = c.call("config.show", json!({}))?;
    let text = result.get("text").and_then(|v| v.as_str()).unwrap_or("");

    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let tmp = std::env::temp_dir().join(format!("typioctl.{}.toml", std::process::id()));
    std::fs::write(&tmp, text)?;
    let status = Command::new(&editor).arg(&tmp).status()?;
    if !status.success() {
        let _ = std::fs::remove_file(&tmp);
        return Err(io::Error::other(format!(
            "{editor} exited with status {status}"
        )));
    }
    let new_text = std::fs::read_to_string(&tmp)?;
    let _ = std::fs::remove_file(&tmp);
    if new_text == text {
        if out == OutputFormat::Plain {
            eprintln!("typioctl: config unchanged");
        }
        return Ok(());
    }
    /* Whole-file replacement isn't part of TIP v1 (config.set takes typed
     * keys, not raw text). For now, edit is read-only with a notice; users
     * who need bulk edits must do `config set key value` per change. */
    eprintln!(
        "typioctl: editor closed with changes, but TIP v1 has no whole-file write — \
               please apply edits via `config set <key> <value>` per change."
    );
    if out == OutputFormat::Json {
        print_json(&json!({ "applied": false, "reason": "whole-file write unsupported" }))?;
    }
    Ok(())
}

/* ---------------------------------------------------------------- */
/* daemon.*                                                         */
/* ---------------------------------------------------------------- */

pub fn daemon_status(out: OutputFormat) -> io::Result<()> {
    let mut c = connect()?;
    let result = c.call("daemon.status", json!({}))?;
    if out == OutputFormat::Json {
        return print_json(&result);
    }
    print_kv(&result, "");
    Ok(())
}

pub fn daemon_stop(out: OutputFormat) -> io::Result<()> {
    let mut c = connect()?;
    let result = c.call("daemon.stop", json!({}))?;
    if out == OutputFormat::Json {
        return print_json(&result);
    }
    println!("Daemon stop requested.");
    Ok(())
}

pub fn daemon_version(out: OutputFormat) -> io::Result<()> {
    let mut c = connect()?;
    let result = c.call("daemon.version", json!({}))?;
    if out == OutputFormat::Json {
        return print_json(&result);
    }
    if let Some(v) = result.get("version").and_then(|v| v.as_str()) {
        println!("{v}");
    }
    Ok(())
}

fn print_kv(value: &Value, indent: &str) {
    match value {
        Value::Object(map) => {
            let key_width = map.keys().map(|k| k.len()).max().unwrap_or(0);
            for (k, v) in map {
                match v {
                    Value::Object(_) => {
                        println!("{indent}{k}:");
                        let deeper = format!("{indent}  ");
                        print_kv(v, &deeper);
                    }
                    Value::Array(arr) => {
                        let joined: Vec<String> = arr.iter().map(value_inline).collect();
                        println!(
                            "{indent}{k:width$}  [{}]",
                            joined.join(", "),
                            width = key_width
                        );
                    }
                    _ => println!("{indent}{k:width$}  {}", value_inline(v), width = key_width),
                }
            }
        }
        _ => println!("{indent}{}", value_inline(value)),
    }
}

fn value_inline(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}
