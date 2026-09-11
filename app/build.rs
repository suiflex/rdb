fn main() {
    // The Slint compiler recurses over the UI's expression tree. Windows'
    // 1 MB default main-thread stack overflows on a UI this deep (Linux/macOS
    // default to 8 MB and are fine), so compile on a thread with a large,
    // explicit stack. Harmless on every platform.
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            // `@material` is Slint's Material 3 component set, vendored from
            // the slint v1.17.1 tag so it always matches the compiler version.
            let manifest_dir = std::path::PathBuf::from(
                std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"),
            );
            let library_paths = std::collections::HashMap::from([(
                "material".to_string(),
                manifest_dir.join("vendor/material/material.slint"),
            )]);
            let config =
                slint_build::CompilerConfiguration::new().with_library_paths(library_paths);
            slint_build::compile_with_config("src/ui/app-window.slint", config)
                .expect("Slint build failed");
        })
        .expect("spawn Slint build thread")
        .join()
        .expect("Slint build thread panicked");
}
