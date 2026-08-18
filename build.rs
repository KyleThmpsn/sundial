fn main() {
    println!("cargo:rerun-if-changed=assets/sundial-alt.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_icon("assets/sundial-alt.ico")
            .set("ProductName", "Sundial")
            .set(
                "FileDescription",
                "Project Sunrise character and loadout settings editor",
            )
            .set("OriginalFilename", "Sundial.exe")
            .set("LegalCopyright", "Copyright © 2026 Kyle Thompson")
            .compile()
            .expect("failed to embed the Sundial Windows resources");
    }
}
