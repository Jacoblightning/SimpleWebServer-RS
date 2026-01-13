// Allowing conditional unstable features

fn main() {
    println!("cargo::rustc-check-cfg=cfg(on_nightly)");
    if rustversion::cfg!(nightly) {
        println!("cargo:rustc-cfg=on_nightly");
    }
}
