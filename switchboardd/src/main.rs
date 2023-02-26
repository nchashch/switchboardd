use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use std::sync::{
    mpsc,
    mpsc::{Receiver, Sender},
};
use switchboard::{config::Config, launcher::*};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(short, long)]
    datadir: Option<PathBuf>,
    #[arg(short, long)]
    bin_download_url: Option<String>,
}

fn main() -> Result<()> {
    let args = Cli::parse();
    let home_dir = dirs::home_dir().unwrap();
    let datadir = args
        .datadir
        .unwrap_or_else(|| home_dir.join(".switchboard"));
    let config: Config = confy::load_path(datadir.join("config.toml"))?;
    let url = args
        .bin_download_url
        .unwrap_or("http://drivechain.info/releases/bin/bin.tar.gz".to_string());
    let mut daemons = Daemons::start(&url, &datadir, &config)?;
    let (tx, rx): (Sender<()>, Receiver<()>) = mpsc::channel();
    ctrlc::set_handler(move || {
        tx.send(()).unwrap();
    })
    .expect("Error setting Ctrl-C handler");
    rx.recv()?;
    Ok(())
}
