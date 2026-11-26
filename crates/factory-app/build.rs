fn main() {
    println!("cargo:rerun-if-changed=src/macos_os_log.c");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("src/macos_os_log.c")
            .flag("-Wall")
            .flag("-Wextra")
            .compile("bruk_macos_os_log");
    }
}
