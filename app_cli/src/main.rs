//! CLI application for room.rs

use anyhow::Result;
use clap::Parser;
use core::Error;
use log::{debug, error, info, trace, warn};

/// room.rs - Secure, spatial audio chat
#[derive(Parser, Debug)]
#[clap(author, version, about)]
struct Args {
    /// Enable debug logging
    #[clap(short, long)]
    debug: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Configure logging based on debug flag
    if args.debug {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug")).init();
        debug!("Debug logging enabled");
    } else {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    }

    info!("Starting room.rs CLI");

    // Will be implemented in Phase 1
    println!("room.rs - Secure, spatial audio chat");
    println!("Phase 0 complete - Basic structure implemented");

    Ok(())
}
