//! Privileged helper daemon: reads /dev/usbmonN, filters the Snap button
//! status endpoint of the microscope and publishes events on a Unix socket.
//!
//! Stub for M0; implemented in M9 (docs/ARCHITECTURE.md §6.10).

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::warn!("hiview-buttond is not implemented yet (scheduled for M9)");
}
