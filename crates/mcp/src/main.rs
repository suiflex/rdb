use std::error::Error;

fn install_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

fn main() -> Result<(), Box<dyn Error>> {
    install_crypto_provider();
    rdb_core::conn::set_client_id(format!("RDB {}", env!("CARGO_PKG_VERSION")));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(rdb_mcp::run())
}
