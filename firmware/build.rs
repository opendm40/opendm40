// Relink when the linker script changes (cargo does not track -T files).
fn main() {
    println!("cargo:rerun-if-changed=link-native.x");
}
