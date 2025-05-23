use std::{fs, ops::Sub, path::Path};

use jiff::{SignedDuration, Timestamp, Zoned, civil::Time};
use serde::{Deserialize, Serialize};
use tracing::{debug, error};

const FILE_NAME: &str = "boops.toml";

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct BoopStorage {
    /// Lifetime boops
    total_boops: u64,

    /// Today's boops
    ///
    /// Resets on midnight, local TZ
    today_boops: u32,

    /// Highest daily boops achieved
    today_boops_record: u32,

    /// Yesterday's boops
    yesterday_boops: u32,

    /// Last reset
    #[serde(default = "today_midnight")]
    last_reset: Zoned,

    /// Last time our boop storage got saved
    #[serde(skip)]
    last_save: Timestamp,
}

impl Default for BoopStorage {
    fn default() -> Self {
        BoopStorage {
            total_boops: 0,
            today_boops: 0,
            today_boops_record: 0,
            yesterday_boops: 0,
            last_reset: today_midnight(),
            last_save: Timestamp::now(),
        }
    }
}

impl BoopStorage {
    /// Load or create boop stats
    pub(crate) fn load() -> Self {
        let file = Path::new(FILE_NAME);

        if file.exists() {
            // try to read existing config
            let contents = match fs::read_to_string(file) {
                Ok(contents) => contents,
                Err(e) => {
                    error!(err=%e, "failed to read {FILE_NAME}");
                    return BoopStorage::default();
                }
            };

            // parse contents or return to defaults
            return toml::from_str::<BoopStorage>(&contents).unwrap_or_else(|e| {
                error!(err=%e, "failed to parse {FILE_NAME}, reverting to defaults");
                BoopStorage::default()
            });
        }

        BoopStorage::default()
    }

    /// Save boop stats
    pub(crate) fn save(&mut self) {
        let toml = match toml::to_string(&self) {
            Ok(toml) => toml,
            Err(e) => {
                error!(err=%e, "failed to serialize boop storage to string");
                panic!();
            }
        };

        if let Err(e) = fs::write(FILE_NAME, toml) {
            error!(err=%e, "failed to write boop storage to {FILE_NAME}");
            return;
        }

        self.last_save = Timestamp::now();
        debug!("saved boop stats: {:?}", self);
    }

    /// Check if storage should be saved again
    pub(crate) fn time_to_save(&self) -> bool {
        let now = Timestamp::now();
        self.last_save < now.sub(SignedDuration::from_mins(5))
    }

    pub(crate) fn inc_boops(&mut self) {
        self.check_reset();

        self.today_boops += 1;
        self.total_boops += 1;

        if self.today_boops > self.today_boops_record {
            self.today_boops_record = self.today_boops;
        }

        if self.time_to_save() {
            self.save();
        }
    }

    pub(crate) fn boop_numbers(&self) -> (u32, u64) {
        (self.today_boops, self.total_boops)
    }

    /// Check if today's boops should be reset
    fn check_reset(&mut self) {
        // reset today's boops, copy to yesterday if past midnight
        let now = Zoned::now();
        if time_is_past_midnight(&self.last_reset, &now) {
            self.yesterday_boops = self.today_boops;
            self.today_boops = 0;
            self.last_reset = now;
            self.save();
        }
    }
}

/// Get midnight of today
fn today_midnight() -> Zoned {
    Zoned::now()
        .with()
        .time(Time::midnight())
        .build()
        .expect("failed to create midnight")
}

/// Check if we're past our last reset `date`, assume we're past today's
/// midnight
fn time_is_past_midnight(last_reset: &Zoned, time: &Zoned) -> bool {
    // date hasn't rolled over
    time.date() != last_reset.date()
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    #[test]
    fn test_time_is_past_midnight() {
        let last_reset = Zoned::from_str("2025-03-30T00:00:00Z[Europe/Berlin]").unwrap();
        let now = last_reset.with().hour(23).minute(59).build().unwrap();

        assert!(!time_is_past_midnight(&last_reset, &now));

        let now2 = Zoned::from_str("2025-03-31T00:00:00Z[Europe/Berlin]").unwrap();
        assert!(time_is_past_midnight(&last_reset, &now2));
    }
}
