use std::{fs, path::Path};

use clap::Parser;
use serde::{Deserialize, Serialize};
use serde_valid::{
    Validate,
    toml::{FromTomlStr, ToTomlString},
};
use tracing::error;

const FILE_NAME: &str = "config.toml";

/// Send one or many messages to a UDP-based OSC-accepting socket
#[derive(Parser, Debug, Serialize, Deserialize)]
#[command(version, about, long_about = None)]
pub(crate) struct Cli {
    /// Port to send to [default: 9000]
    #[arg(short, long, value_parser=clap::value_parser!(u16).range(1024..))]
    send: Option<u16>,

    /// Create config.toml with specified/default values
    #[arg(long, default_value_t = false)]
    save: bool,

    /// OSC parameter suffix for boops [default: /OSCBoop]
    ///
    /// Matching is done via str.ends_with({boop_address})
    #[arg(short, long, value_parser=clap::value_parser!(String))]
    boop_address: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Validate)]
pub(crate) struct Options {
    #[validate(minimum = 0)]
    #[serde(default = "default_osc_send_port")]
    pub osc_send_port: u16,

    #[serde(default = "default_boop_address")]
    pub boop_address: String,

    #[serde(
        default = "default_text_suffixes",
        deserialize_with = "deserialize_text_suffixes"
    )]
    #[validate]
    pub text_suffixes: Vec<TextSuffix>,
}

#[derive(Debug, PartialEq, Serialize, Validate)]
pub(crate) struct TextSuffix {
    /// arithmetic remainder
    #[validate(minimum = 0)]
    value: u64,

    /// string to append to chatbox message
    message: String,

    #[serde(skip)]
    /// divisor for arithmetic remainder calculation
    divisor: u128,
}

#[derive(Debug, PartialEq)]
pub enum TextSuffixResult {
    /// lookup loop should break
    Break,

    /// message matches, send
    Message(String),

    /// does not match
    Skip,
}

impl Options {
    pub(crate) fn new() -> Self {
        let args = Cli::parse();

        // try to load config/init with args/defaults
        let mut options = Options::load();

        // override values again, if specified
        if let Some(send) = args.send {
            options.osc_send_port = send;
        }
        if let Some(boop_address) = args.boop_address {
            options.boop_address = boop_address;
        }

        // save new config
        if args.save {
            options.save();
        }

        options
    }

    /// Load config if it exists
    fn load() -> Self {
        let file = Path::new(FILE_NAME);

        if !file.exists() {
            return Options::default();
        }

        // try to read existing config
        let contents = match fs::read_to_string(file) {
            Ok(contents) => contents,
            Err(e) => {
                error!(err=%e, "failed to read {FILE_NAME}");
                return Options::default();
            }
        };

        let options = Options::from_toml_str(&contents)
            .map_err(|e| {
                error!(err=%e, "failed to parse {FILE_NAME}");
            })
            .unwrap_or_default();

        options
            .validate()
            .map_err(|errors| {
                error!("failed to validate config file {FILE_NAME}: {errors}");
            })
            .unwrap(); // crash intentionally

        options
    }

    /// Save config
    fn save(&self) {
        let toml = match self.to_toml_string() {
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

impl Default for Options {
    fn default() -> Self {
        Options {
            osc_send_port: 9000,
            boop_address: default_boop_address(),
            text_suffixes: default_text_suffixes(),
        }
    }
}

impl TextSuffix {
    pub(crate) fn new(value: u64, message: String) -> Self {
        TextSuffix {
            value,
            message,
            divisor: TextSuffix::calculate_divisor(value),
        }
    }

    /// check for number match, or break
    pub(crate) fn check_value(&self, value: u64) -> TextSuffixResult {
        if value < self.value {
            return TextSuffixResult::Break;
        }

        if value as u128 % self.divisor == self.value as u128 {
            return TextSuffixResult::Message(self.message.clone());
        }

        TextSuffixResult::Skip
    }

    /// calculate appropriate divisor via log10
    pub(crate) fn calculate_divisor(value: u64) -> u128 {
        let logged_value = value.ilog10();

        // 10^(n+1) results in appropriate value
        let base: u128 = 10;
        base.pow(logged_value + 1)
    }
}

/// deserialize and sort [`Vec<TextSuffix>`] by value
fn deserialize_text_suffixes<'de, D>(deserializer: D) -> Result<Vec<TextSuffix>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let mut vec = Vec::<TextSuffix>::deserialize(deserializer)?;
    vec.sort_by_key(|o| o.value);

    Ok(vec)
}

/// deserialize [`TextSuffix`] and calculate divisor immediately
impl<'de> Deserialize<'de> for TextSuffix {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Helper {
            value: u64,
            message: String,
        }

        let helper = Helper::deserialize(deserializer)?;
        Ok(TextSuffix {
            value: helper.value,
            message: helper.message,
            divisor: TextSuffix::calculate_divisor(helper.value),
        })
    }
}

fn default_osc_send_port() -> u16 {
    9000
}

fn default_boop_address() -> String {
    "/OSCBoop".into()
}

fn default_text_suffixes() -> Vec<TextSuffix> {
    vec![
        TextSuffix::new(69, "Nice".into()),
        TextSuffix::new(420, "Blaze it".into()),
        TextSuffix::new(621, "owo".into()),
        TextSuffix::new(666, "ooOooO scary".into()),
        TextSuffix::new(1337, "much leet so wow".into()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_suffix_deserialisation() {
        let content = r#"
        value = 69
        message = "Nice"
        "#;

        let text_suffix = TextSuffix::from_toml_str(content).unwrap();

        assert_eq!(
            text_suffix,
            TextSuffix {
                value: 69,
                message: "Nice".into(),
                divisor: 100
            }
        )
    }

    #[test]
    fn test_text_suffix_response() {
        let suffix = TextSuffix::new(69, "Nice".into());

        assert_eq!(
            suffix.check_value(12345669),
            TextSuffixResult::Message("Nice".into())
        );
    }
}
