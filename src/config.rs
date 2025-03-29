use std::{fs, path::Path};

use clap::Parser;
use log::error;
use serde::{Deserialize, Serialize};

const FILE_NAME: &str = "config.toml";

/// Send one or many messages to a UDP-based OSC-accepting socket
#[derive(Parser, Debug, Serialize, Deserialize)]
#[command(version, about, long_about = None)]
pub(crate) struct Cli {
    /// Port to listen to [default: 9001]
    #[arg(short, long)]
    listen: Option<u16>,

    /// Port to send to [default: 9000]
    #[arg(short, long)]
    send: Option<u16>,

    /// Create config.toml with specified/default values
    #[arg(long, default_value_t = false)]
    save: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Options {
    pub listen: u16,
    pub send: u16,
}

impl Options {
    pub(crate) fn new() -> Self {
        let args = Cli::parse();

        // try to load config/init with args/defaults
        let mut options = Options::load().unwrap_or({
            Options {
                listen: args.listen.unwrap_or(9001),
                send: args.send.unwrap_or(9000),
            }
        });

        // override values again, if specified
        if let Some(listen) = args.listen {
            options.listen = listen;
        }
        if let Some(send) = args.send {
            options.send = send;
        }

        if options.listen == options.send {
            error!("Listen and send port may not be identical");
            std::process::exit(1);
        }

        // save new config
        if args.save {
            options.save();
        }

        options
    }

    /// Load config if it exists
    pub(crate) fn load() -> Option<Self> {
        let file = Path::new(FILE_NAME);

        if !file.exists() {
            return None;
        }

        // try to read existing config
        let contents = match fs::read_to_string(file) {
            Ok(contents) => contents,
            Err(e) => {
                error!("failed to read {}: {}", FILE_NAME, e);
                return None;
            }
        };

        // parse contents or return to defaults
        match toml::from_str::<Options>(&contents) {
            Ok(options) => Some(options),
            Err(e) => {
                error!("failed to parse {}: {}", FILE_NAME, e);
                None
            }
        }
    }

    /// Save config
    pub(crate) fn save(&self) {
        let toml = match toml::to_string(&self) {
            Ok(toml) => toml,
            Err(e) => {
                error!("failed to serialize config to string: {}", e);
                panic!();
            }
        };

        if let Err(err) = fs::write(FILE_NAME, toml) {
            error!("failed to write to {}: {}", FILE_NAME, err);
        }
    }
}
