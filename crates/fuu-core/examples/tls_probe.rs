//! Diagnostic: open an IMAP connection (TLS handshake + greeting only, no login).
//! `cargo run -p fuu-core --example tls_probe -- imap.example.com 993 [starttls]`
use fuu_core::account::{Security, ServerConfig};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut args = std::env::args().skip(1);
    let host = args.next().expect("host");
    let port: u16 = args.next().map(|p| p.parse().expect("port")).unwrap_or(993);
    let security = if args.next().as_deref() == Some("starttls") { Security::StartTls } else { Security::Tls };
    let server = ServerConfig { host: host.clone(), port, security };
    match fuu_core::imap::connect(&server).await {
        Ok(_) => println!("OK: TLS established with {host}:{port}"),
        Err(e) => println!("FAILED: {e}"),
    }
}
