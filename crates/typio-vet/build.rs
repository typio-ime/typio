fn main() {
    // Export #[unsafe(no_mangle)] symbols so that dlopen'd engine plugins can
    // resolve mock libtypio functions (typio_input_context_commit, etc.)
    // against this binary.
    println!("cargo:rustc-link-arg=-rdynamic");
}
