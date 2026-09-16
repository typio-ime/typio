# Retired contract layers

The former installed C header layers (`typio/abi`, `typio/runtime`, and
`typio/schema`) were removed by
[ADR-0046](../../../../adr/0046-engine-protocol-only-runtime.md).

The current boundaries are:

| Boundary | Contract |
|---|---|
| Daemon to engine | Typed Typio Engine Protocol over private fd 3 |
| Daemon to external client | TIP over a credential-checked Unix socket |
| Host modules to runtime | Owned Rust APIs inside the workspace |

No allocator, pointer, struct-layout, symbol-version, header, or dynamic-loader
contract crosses these boundaries.
