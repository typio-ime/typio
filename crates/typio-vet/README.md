# typio-vet

Conformance vetting for native Typio C ABI engine artifacts.

`typio-vet` loads a single native C ABI engine artifact into the vet process and
puts it through three dimensions of the engine contract, reporting a `PASS` /
`WARN` / `FAIL` verdict per check:

| Dimension | What it covers |
|-----------|----------------|
| **ABI** | `TypioEngineInfo` fields, vtable completeness |
| **Behavior** | invariants observed by driving the engine against a mock host |
| **Resource** | packaged assets that ship beside the native engine artifact — today the freedesktop icon contract |

Only `FAIL` blocks the gate (non-zero exit). `WARN` flags behavior that is
*legal but suspicious* — for example a keyboard engine that declines a plain
ASCII key — so a do-nothing engine cannot quietly pass clean.

## Quick start

```bash
cargo run -p typio-vet --bin typio-vet -- \
    ../typio-engine-basic/target/debug/libtypio_engine_basic.so
```

```
typio-vet: .../libtypio_engine_basic.so (name=basic, type=TypioEngineTypeKeyboard)
           package: ../typio-engine-basic

  ABI
    create .................. PASS
    info_present ............ PASS
    name .................... PASS
    type_matches_slot ....... PASS
    base_vtable ............. PASS
    keyboard_vtable ......... PASS
    process_key_present ..... PASS

  Behavior
    lifecycle ............... PASS
    printable_key ........... PASS
    modifier_passthrough .... PASS
    escape_on_empty ......... PASS
    focus_churn ............. PASS
    reset ................... PASS
    config_reload ........... PASS

  Resource
    icon_name ............... PASS
    icon_asset .............. PASS
    svg_wellformed .......... PASS

23 passed, 0 warnings, 0 failed
```

### Options

```
typio-vet <engine-abi.so> [options]

    --package <dir>    Package root for resource checks (auto-detected otherwise)
    --only <dims>      Comma-separated: abi, behavior, resource
    --check <name>     Run/report only the named check
    --list             List the dimensions and exit
    --help, -h         Show usage
```

```bash
# ABI + resources only
typio-vet ./libtypio_engine_rime.so --only abi,resource

# point at a package root explicitly (e.g. when the artifact lives in target/)
typio-vet ./libtypio_engine_whisper.so --package ../typio-engine-whisper
```

## Using in Rust engine tests

`typio-vet` doubles as a mock host you can link as a dev-dependency. It exports
the `typio_*` host symbols (`typio_input_context_commit`, the config getters,
…) so an engine built into the test binary resolves them against the mock and
records every side effect.

```toml
[dev-dependencies]
typio-vet = { path = "crates/typio-vet" }
```

```rust
use typio_vet::{key_press, ContextEvent, TestHarness, TypioKeyProcessResult};

#[test]
fn engine_commits_a() {
    let mut h = unsafe {
        TestHarness::new_keyboard(typio_keyboard_engine_create, Default::default())
    }
    .expect("init failed");

    let r = unsafe { h.press(&key_press('a')) };
    assert_eq!(r, TypioKeyProcessResult::TypioKeyCommitted);
    assert_eq!(h.log.take(), vec![ContextEvent::Commit("a".into())]);

    unsafe { h.destroy() };
}
```

See [`examples/demo_engine.rs`](examples/demo_engine.rs) for a complete engine
plus its tests.

## Checks

### ABI

| Check | Verdict on miss |
|-------|-----------------|
| `create` / `info_present` / `base_vtable` | FAIL — engine is unusable |
| `name` / `type_matches_slot` / `process_key_present` (kbd) / `process_audio_present` (voice) | FAIL |
| `display_name` / `author` / `language` / `init_present` / `destroy_present` | WARN |

### Behavior (keyboard)

| Check | Contract |
|-------|----------|
| `lifecycle` | `init` → `destroy` succeeds |
| `printable_key` | result code and side effects cohere; a commit emits non-empty text; declining `'a'` is a WARN |
| `modifier_passthrough` | a lone modifier never commits |
| `escape_on_empty` | Escape on an empty context never commits |
| `focus_churn` | `focus_in`→`focus_out`→`focus_in` then still processes a key |
| `reset` | `reset` clears composition, never commits |
| `config_reload` | `reload_config` returns OK |

### Behavior (voice)

| Check | Contract |
|-------|----------|
| `lifecycle` | `init` → `destroy` succeeds |
| `is_ready` | reports readiness; "not ready" is a WARN, not a failure |
| `process_audio_silent` | 1s of silence does not crash; any returned text is valid UTF-8 |

### Resource

| Check | Contract |
|-------|----------|
| `icon_name` | `TypioEngineInfo.icon` is a bare freedesktop icon *name*, not a path or filename |
| `icon_asset` | a matching `<name>.svg` or `<name>-symbolic.svg` exists under `data/icons/hicolor/**/apps/` (or the bundled `icons/hicolor/**`) |
| `svg_wellformed` | the SVG is valid UTF-8, has an `<svg>` root with `viewBox` or width/height, and balanced tags |
| `icon_placement` | a `-symbolic` asset lives under `symbolic/` or `scalable/` (WARN otherwise) |

## Scope

`typio-vet` vets **native C ABI engine artifacts** by loading them in the vet
process. It is not a host-process manifest/executable vetter, and it is not a
test harness for hosts, the CLI, or settings; those are tested with ordinary
`cargo test`.
