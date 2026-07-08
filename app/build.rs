fn main() {
    // The Slint compiler recurses over the UI's expression tree. Windows'
    // 1 MB default main-thread stack overflows on a UI this deep (Linux/macOS
    // default to 8 MB and are fine), so compile on a thread with a large,
    // explicit stack. Harmless on every platform.
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            slint_build::compile("src/ui/app-window.slint").expect("Slint build failed");
        })
        .expect("spawn Slint build thread")
        .join()
        .expect("Slint build thread panicked");
}
