mod cert;
mod server;

use std::{env, path::PathBuf};

use base64::Engine;
use eyre::{bail, Context, Result};
use tokio::net::UnixListener;
use tonic::transport::{Certificate, ServerTlsConfig};
use tracing::{info, Level};

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .with_writer(std::io::stderr)
        .without_time()
        .init();

    let client_cert =
        std::env::var("PLUGIN_CLIENT_CERT").wrap_err("PLUGIN_CLIENT_CERT not found")?;
    let client_cert = Certificate::from_pem(client_cert);
    let (server_identity, server_cert) =
        cert::generate_cert().wrap_err("generating server certificate")?;

    let (_tmpdir, socket) = match init_handshake(&server_cert).await {
        Ok(addr) => addr,
        Err(err) => {
            println!("{:?}", err);
            bail!("init error");
        }
    };

    let tls = ServerTlsConfig::new()
        .identity(server_identity)
        .client_auth_optional(true) // ??? terraform doesn't send certs ???
        .client_ca_root(client_cert);


    info!("Listening on {}", socket.display());

    let uds = UnixListener::bind(socket).wrap_err("failed to bind unix listener")?;
    let uds_stream = tokio_stream::wrappers::UnixListenerStream::new(uds);

    tonic::transport::Server::builder()
        .tls_config(tls)
        .wrap_err("invalid TLS config")?
        .add_service(server::tfplugin6::provider_server::ProviderServer::new(
            server::MyProvider,
        ))
        .serve_with_incoming(uds_stream)
        .await
        .wrap_err("failed to start server")?;

    Ok(())
}

const _MAGIC_COOKIE_KEY: &str = "TF_PLUGIN_MAGIC_COOKIE";
const _MAGIC_COOKIE_VALUE: &str =
    "d602bf8f470bc67ca7faa0386276bbdd4330efaf76d1a219cb4d6991ca9872b2";

async fn init_handshake(server_cert: &rcgen::Certificate) -> Result<(tempfile::TempDir, PathBuf)> {
    // https://github.com/hashicorp/go-plugin/blob/8d2aaa458971cba97c3bfec1b0380322e024b514/docs/internals.md

    let _ = env::var("PLUGIN_MIN_PORT")
        .wrap_err("PLUGIN_MIN_PORT not found")?
        .parse::<u16>()
        .wrap_err("PLUGIN_MIN_PORT not an int")?;
    let _ = env::var("PLUGIN_MAX_PORT")
        .wrap_err("PLUGIN_MAX_PORT not found")?
        .parse::<u16>()
        .wrap_err("PLUGIN_MAX_PORT not an int")?;

    let tmpdir = tempfile::TempDir::new().wrap_err("failed to create temporary directory")?;
    let socket = tmpdir.path().join("plugin");

    // https://github.com/hashicorp/go-plugin/blob/8d2aaa458971cba97c3bfec1b0380322e024b514/server.go#L426
    const CORE_PROTOCOL_VERSION: u8 = 1;
    const PROTO_VERSION: u8 = 6;
    let listener_addr_network = "unix";
    let listener_addr = socket.display();
    let proto_type = "grpc";
    let b64_cert = base64::prelude::BASE64_STANDARD_NO_PAD.encode(server_cert.der());

    println!("{CORE_PROTOCOL_VERSION}|{PROTO_VERSION}|{listener_addr_network}|{listener_addr}|{proto_type}|{b64_cert}");

    Ok((tmpdir, socket))
}
