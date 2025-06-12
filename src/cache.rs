use chrono::{Local, NaiveDateTime};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::io::Write;
use std::{fs::File, io::Result, path::PathBuf};

use crate::church::ChurchClient;
use crate::env::Env;
use crate::holly::scheduled_times::RefetchPolicy;
use crate::persons::Person;

#[derive(Serialize, Deserialize, Debug)]
pub struct PersonsCacheMap(pub HashMap<String, PersonCache>);

impl PersonsCacheMap {
    pub fn get_cache(env: Env) -> Result<Self> {
        let mut file_path = PathBuf::from(&env.working_path);

        file_path.push("cache.json");

        if !file_path.exists() {
            let mut file = File::create(&file_path)?;
            file.write_all(b"{}")?;
            return Ok(PersonsCacheMap(HashMap::new()));
        }

        let file = File::open(file_path)?;
        let map: PersonsCacheMap = serde_json::from_reader(file)?;
        Ok(map)
    }

    pub fn save_cache(&self, env: Env) -> Result<()> {
        let file_path = PathBuf::from(&env.working_path);

        let file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(file_path.join("cache.json"))?;

        serde_json::to_writer(file, &json!(self))?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PersonCache {
    pub response_time: Option<usize>,
    pub has_attended_since_last_referral: bool,
    pub has_attended_sacrament: bool,
    pub sacrament_attendance_count: usize,
    pub has_been_taught: bool,
    pub has_been_taught_in_person: bool,
    pub has_been_taught_since_last_referral: bool,
    pub last_timeline_assessment: NaiveDateTime,
    pub referral_count: Option<usize>,
}

impl PersonCache {
    pub async fn get_cached_data_for_person(
        church_client: &mut ChurchClient,
        person: &Person,
        cache_map: &mut PersonsCacheMap,
        refetch_policy: RefetchPolicy,
    ) -> anyhow::Result<Option<Self>> {
        use RefetchPolicy::*;

        match refetch_policy {
            ForceRefetch => return Self::update_cache(church_client, person, cache_map).await,
            NoRefetch => {
                if let Some(cache) = cache_map.0.get(&person.guid) {
                    return Ok(Some(cache.clone()));
                } else {
                    return Ok(None);
                }
            }
            RefetchAfter(_) => {
                if let Some(cache) = cache_map.0.get(&person.guid) {
                    // check to see if cache has been made since the last assignment time
                    // (this is so we catch duplicate referrals and get latest timeline stats)
                    if person.assigned_date <= cache.last_timeline_assessment {
                        return Ok(Some(cache.clone()));
                    }
                }
            }
        }
    
        Self::update_cache(church_client, person, cache_map).await
    }

    async fn update_cache(
        church_client: &mut ChurchClient,
        person: &Person,
        cache_map: &mut PersonsCacheMap,
    ) -> anyhow::Result<Option<Self>> {
        if let Ok(person_cache) = Self::retrieve_data_to_cache(church_client, person).await {
            cache_map
                .0
                .insert(person.guid.clone(), person_cache.clone());
            return Ok(Some(person_cache.clone()));
        }
        Ok(None)
    }
    async fn retrieve_data_to_cache(
        church_client: &mut ChurchClient,
        person: &Person,
    ) -> anyhow::Result<Self> {
        let mut timeline = church_client.get_person_timeline(&person).await?;
        timeline.reverse();

        let (
            response_time,
            has_attended_sacrament,
            has_attended_since_last_referral,
            sacrament_attendance_count,
            has_been_taught,
            has_been_taught_in_person,
            has_been_taught_since_last_referral,
            referral_count,
        ) = church_client.get_timeline_stats(&timeline).await;

        Ok(Self {
            response_time,
            has_attended_sacrament,
            has_attended_since_last_referral,
            sacrament_attendance_count,
            has_been_taught,
            has_been_taught_in_person,
            has_been_taught_since_last_referral,
            referral_count,
            last_timeline_assessment: Local::now().naive_local(),
        })
    }
}
