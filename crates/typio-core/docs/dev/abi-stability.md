# Retired ABI stability policy

Typio no longer publishes a host or engine binary ABI. The former ABI version,
header-layout, `struct_size`, SONAME, and pkg-config policy was retired by
[ADR-0046](../../../../docs/adr/0046-engine-protocol-only-runtime.md).

Compatibility is now negotiated at framed protocol boundaries. See the
repository [interface stability reference](../../../../docs/reference/stability.md)
and the [`typio-engine-protocol` contract](../../../typio-engine-protocol/README.md).
