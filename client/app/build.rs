fn main() {
    println!("cargo:rerun-if-changed=assets/icons/icon.ico");
    embed_resource::compile("app.rc", embed_resource::NONE)
        .manifest_optional()
        .expect("embed the application icon");
}
