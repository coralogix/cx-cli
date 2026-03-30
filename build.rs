fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Only rerun if the proto files change.
    println!("cargo:rerun-if-changed=proto/com/coralogix/schemastore/v1/olly_service.proto");
    println!("cargo:rerun-if-changed=proto/");

    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    std::env::set_var("PROTOC", protoc);

    tonic_build::configure()
        .build_server(false)
        .compile_protos(
            &["proto/com/coralogix/schemastore/v1/olly_service.proto"],
            &["proto"],
        )?;

    Ok(())
}
