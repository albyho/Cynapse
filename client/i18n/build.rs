fn main() {
    // rust-i18n embeds YAML catalogs during compilation, but Cargo does not
    // infer the locale files from the macro expansion.
    println!("cargo:rerun-if-changed=locales");
}
