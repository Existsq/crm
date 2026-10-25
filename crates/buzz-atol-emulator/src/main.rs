//! `buzz-atol-emulator [--port 16732] [--delay-ms 300] [--shift-open]`
//!
//! Runs the ATOL DTO10 web-server emulator for manual checks of the CRM
//! cash desk without a ККМ.

use std::net::SocketAddr;
use std::time::Duration;

use buzz_atol_emulator::{serve, EmulatorConfig};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let mut port: u16 = 16732;
    let mut config = EmulatorConfig::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => port = args.next().and_then(|v| v.parse().ok()).unwrap_or(port),
            "--delay-ms" => {
                if let Some(ms) = args.next().and_then(|v| v.parse().ok()) {
                    config.processing_delay = Duration::from_millis(ms);
                }
            }
            "--shift-open" => config.shift_open = true,
            other => {
                eprintln!("unknown argument {other}");
                std::process::exit(1);
            }
        }
    }
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    println!("ATOL DTO10 emulator on http://{addr}/api/v2/requests");
    serve(addr, config).await
}
