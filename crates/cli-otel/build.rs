fn main() {
    println!("cargo:rustc-check-cfg=cfg(cli_otel_embedded)");
}
