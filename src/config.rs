use std::{fs, path::Path};

use clap::Parser;
use serde::{Deserialize, Serialize};
use tracing::error;

const FILE_NAME: &str = "config.toml";

/// Send one or many messages to a UDP-based OSC-accepting socket
#[derive(Parser, Debug, Serialize, Deserialize)]
#[command(version, about, long_about = None)]
pub(crate) struct Cli {
    /// Port to send to [default: 9000]
    #[arg(short, long)]
    send: Option<u16>,

    /// Create config.toml with specified/default values
    #[arg(long, default_value_t = false)]
    save: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Options {
    pub send: u16,
}

impl Options {
    pub(crate) fn new() -> Self {
        let args = Cli::parse();

        // try to load config/init with args/defaults
        let mut options = Options::load().unwrap_or({
            Options {
                send: args.send.unwrap_or(9000),
            }
        });

        // override values again, if specified
        if let Some(send) = args.send {
            options.send = send;
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
                error!(err=%e, "failed to read {FILE_NAME}");
                return None;
            }
        };

        // parse contents or return to defaults
        match toml::from_str::<Options>(&contents) {
            Ok(options) => Some(options),
            Err(e) => {
                error!(err=%e, "failed to parse {FILE_NAME}");
                None
            }
        }
    }

    /// Save config
    pub(crate) fn save(&self) {
        let toml = match toml::to_string(&self) {
            Ok(toml) => toml,
            Err(e) => {
                error!(err=%e, "failed to serialize config to string");
                panic!();
            }
        };

        if let Err(e) = fs::write(FILE_NAME, toml) {
            error!(err=%e, "failed to write config to {FILE_NAME}");
        }
    }
}
