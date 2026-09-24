fn main() {
    slint_build::compile("ui/app.slint").unwrap();
    // The shared modules live in the shipping app; rebuild when they move.
    println!("cargo:rerun-if-changed=../src-tauri/src");
}
