// Jackson Coxson

use std::{collections::HashMap, path::PathBuf, str::FromStr};

use anyhow::Context;
use chrono::{Days, Duration, NaiveDateTime, NaiveTime, Weekday, Datelike};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Frequency {
    Daily,
    Weekly,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScheduleEntry {
    pub last: NaiveDateTime,
    pub next: NaiveDateTime,
    pub frequency: Frequency,
    #[serde(with = "time_format")]
    pub send_time: NaiveTime,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub day: Option<Weekday>,
}

// Custom (de)serializer for "HH:MM" time format
mod time_format {
    use chrono::NaiveTime;
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(time: &NaiveTime, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&time.format("%H:%M").to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<NaiveTime, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        NaiveTime::parse_from_str(&s, "%H:%M").map_err(serde::de::Error::custom)
    }
}

pub type SendTimeMap = HashMap<String, ScheduleEntry>;

impl ScheduleEntry {
    pub fn set_next(&mut self) {
        let now = chrono::Local::now().naive_local();
        match self.frequency {
            Frequency::Daily => {
                let base_date = if self.last.date() == now.date() {
                    self.last.date().checked_add_days(Days::new(1)).unwrap()
                } else {
                    now.date()
                };
                self.next = NaiveDateTime::new(base_date, self.send_time);
            }
            Frequency::Weekly => {
                if let Some(target_weekday) = self.day {
                    let mut next_date = now.date();
                    let current_weekday = next_date.weekday();
                    let mut days_ahead = (target_weekday.num_days_from_monday() + 7
                        - current_weekday.num_days_from_monday()) % 7;
                    // If today is the day but time has passed, schedule for next week
                    if days_ahead == 0 && now.time() >= self.send_time {
                        days_ahead = 7;
                    }
                    next_date = next_date + Duration::days(days_ahead as i64);
                    self.next = NaiveDateTime::new(next_date, self.send_time);
                } else {
                    // Fallback: just use today
                    self.next = NaiveDateTime::new(now.date(), self.send_time);
                }
            }
        }
    }

    pub fn is_go_time(&mut self) -> bool {
        let now = chrono::Local::now().naive_local();
        if self.last == self.next {
            self.set_next();
            return false;
        }
        if now > self.next {
            self.last = self.next;
            self.set_next();

            return true;
        }
        false
    }
}

pub struct SendTimeStore;


impl SendTimeStore {
    fn file_path(env: &crate::env::Env) -> anyhow::Result<PathBuf> {
        Ok(PathBuf::from_str(&env.working_path)?.join("scheduled_times.json"))
    }

    pub async fn load(env: &crate::env::Env) -> anyhow::Result<SendTimeMap> {
        let file_path = Self::file_path(env)?;
        if !std::fs::exists(&file_path)? {
            return Ok(HashMap::new());
        }
        let s = std::fs::read_to_string(&file_path)?;
        let map: SendTimeMap = serde_json::from_str(&s)?;
        Ok(map)
    }

    pub async fn save(env: &crate::env::Env, map: &SendTimeMap) -> anyhow::Result<()> {
        let file_path = Self::file_path(env)?;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(file_path)?;
        serde_json::to_writer_pretty(file, map)
            .context("Unable to serialize or write send time to file")?;
        Ok(())
    }
}