fn main() -> Result<(), Box<dyn std::error::Error>> {
    prost_build::compile_protos(&["proto/PublicAggreDealsV3Api.proto"], &["proto/"])?;
    Ok(())
}
