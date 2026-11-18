//! Diagnostic: TLS handshake only, with Fuu's verifier. `-- host [port]`
use std::sync::Arc;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut args = std::env::args().skip(1);
    let host = args.next().expect("host");
    let port: u16 = args.next().map(|p| p.parse().unwrap()).unwrap_or(443);
    let config = fuu_core::tls::client_config().expect("config");
    let connector = tokio_rustls::TlsConnector::from(Arc::clone(&config));
    let tcp = tokio::net::TcpStream::connect((host.as_str(), port)).await.expect("tcp");
    let name = rustls::pki_types::ServerName::try_from(host.clone()).unwrap();
    match tokio::time::timeout(std::time::Duration::from_secs(15), connector.connect(name, tcp)).await {
        Ok(Ok(_)) => println!("ACCEPTED {host}"),
        Ok(Err(e)) => println!("REJECTED {host}: {e}"),
        Err(_) => println!("TIMEOUT {host}"),
    }
}
