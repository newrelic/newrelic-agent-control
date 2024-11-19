use std::io::Result;

fn main() -> Result<()> {
    // compiling protos using path on build time
    prost_build::compile_protos(
        &[
            "./opamp-spec/proto/opamp.proto",
            "./opamp-spec/proto/anyvalue.proto",
        ],
        &["./opamp-spec/proto"],
    )?;
    Ok(())
}
