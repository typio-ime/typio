# Cold Archive

Charter: this directory is **cold storage** for retired records. Nothing here is
edited in place, and nothing here is authoritative: active rules live in
[docs/adr/](../index.md) and the [Architecture Blueprints](../../architecture/index.md).

Automated tools and coding assistants should exclude `**/archive/**` from
routine searches and prompt context ([INV-TEMP-02](../../governance/documentation/core/invariants.md)).
Retired records are never deleted — they are relocated here and marked, so that
a future reader can find out *why an approach failed* rather than rediscovering
it ([INV-TEMP-03](../../governance/documentation/core/invariants.md)).

| Set | Covers | Status of the set |
| :--- | :--- | :--- |
| [Framework core ADRs](framework-core/adr/index.md) | The retired C-ABI framework: engine plugin loading, the composition contract, engine properties, mode reflection, host-managed selection | Historical. The C ABI and plugin loading were retired by [ADR-0046](../../adr/0046-engine-protocol-only-runtime.md) |
| [Framework core dev notes](framework-core/dev/index.md) | The retired installed header layers and ABI stability policy, plus keyboard-status provenance | Historical. Kept because the framework ADRs cite them |
| [Framework core index](framework-core/index.md) | The former runtime crate's documentation hub | Historical |
| [CLI control ADRs](cli-control/index.md) | The standalone `typioctl` repository: independence, binary naming, the resource+verb schema | Historical. Repository separation ended with [ADR-0039](../../adr/0039-cli-workspace-integration.md) |

## See also

- [ADR Index](../index.md) — active decisions, and the registry that points here
- [Living Blueprints](../../architecture/index.md) — where compacted invariants land
