use std::{collections::HashMap, fs, path::PathBuf};

use chrono::{Duration, Utc};
use serde::{de::DeserializeOwned, Serialize};

use crate::{church::ChurchClient, persons};

pub mod all_mission_report_weekly;
pub mod zone_report_nightly;
pub mod zone_report_daily;

 pub fn load_reports<T: DeserializeOwned + Default>(
    env: &crate::env::Env,
    filename: &str,
) -> anyhow::Result<T> {
    let today = chrono::Local::now();
    let today_str = today.format("%Y-%m-%d").to_string();
    let mut dir_path = PathBuf::from(&env.working_path);
    dir_path.push("reports");
    dir_path.push(&today_str);

    let mut file_path = dir_path.clone();
    file_path.push(filename);

    if !file_path.exists() {
        return Ok(T::default());
    }

    let file = std::fs::File::open(file_path)?;
    let map = serde_json::from_reader(file)?;
    Ok(map)
}

pub fn save_report<T: Serialize>(
    env: &crate::env::Env,
    filename: &str,
    report: &T,
) -> anyhow::Result<()> {
    let today = chrono::Local::now();
    let today_str = today.format("%Y-%m-%d").to_string();
    let mut dir_path = PathBuf::from(&env.working_path);
    dir_path.push("reports");
    dir_path.push(&today_str);

    fs::create_dir_all(&dir_path)?;
    let mut file_path = dir_path.clone();
    file_path.push(filename);

    let file = fs::File::create(file_path)?;
    serde_json::to_writer_pretty(file, report)?;
    Ok(())
}

pub fn get_zone_name_from_id(env: &crate::env::Env, zone_id: usize) -> Option<String> {
    let file_path = PathBuf::from(&env.working_path).join("zone_ids.json");

    if !file_path.exists() {
        // do something eventually
    }

    let file = std::fs::File::open(file_path).unwrap();
    let zones = serde_json::from_reader::<_, HashMap<usize, String>>(file)
        .expect("Failed to read zones from file");

    zones.get(&zone_id).cloned()
}

pub async fn get_templates(env: &crate::env::Env) -> anyhow::Result<HashMap<String, String>> {
    let file_path = PathBuf::from(&env.working_path).join("message_templates.json");
    if !file_path.exists() {
        return Err(anyhow::anyhow!("Templates file not found at {:?}", file_path));
    }

    let file = std::fs::File::open(file_path)?;
    let templates = serde_json::from_reader(file).unwrap();
    
    Ok(templates)

}
pub async fn get_average(
    church_client: &mut ChurchClient,
    requested_zone: Option<String>,
    require_refetch: bool
) -> anyhow::Result<HashMap<String, (usize, usize)>> {
    let mut contacts = church_client.env.load_contacts()?;

    let persons_list = church_client.get_cached_people_list().await?.to_vec();
    let now = Utc::now().naive_utc();
    let persons_list: Vec<persons::Person> = persons_list
        .into_iter()
        .filter(|x| {
            x.referral_status != persons::ReferralStatus::NotAttempted
                && (x.person_status < persons::PersonStatus::NewMember)
                && now.signed_duration_since(x.assigned_date) < Duration::hours(24)
        })
        .collect();

    let mut zones = HashMap::new();
    // let bar = ProgressBar::new(persons_list.len() as u64);
    for person in persons_list {
        if let Some(zone_name) = &person.zone_name {
            // bar.inc(1);
           
            let t: usize = if require_refetch {
                // Always fetch and update cache
                if let Some(contact_time) = church_client.get_person_contact_time(&person).await? {
                    contacts.insert(person.guid.clone(), contact_time);
                    contact_time
                } else {
                    continue;
                }
            } else {
                // Try cache first, then fetch if missing
                match contacts.get(&person.guid).cloned() {
                    Some(t) => t,
                    None => {
                        if let Some(contact_time) = church_client.get_person_contact_time(&person).await? {
                            contacts.insert(person.guid.clone(), contact_time);
                            contact_time
                        } else {
                            continue;
                        }
                    }
                }
            };
           

            let zone = match zones.get_mut(zone_name) {
                Some(z) => z,
                None => {
                    zones.insert(zone_name.clone(), HashMap::new());
                    zones.get_mut(zone_name).unwrap()
                }
            };
            if let Some(area_name) = &person.area_name {
                let area = match zone.get_mut(area_name) {
                    Some(n) => n,
                    None => {
                        zone.insert(area_name.clone(), (0 as usize, 0 as usize));
                        zone.get_mut(area_name).unwrap()
                    }
                };
             
                area.0 += 1;
                area.1 += t;
            }
        }    
    }

    church_client.env.save_contacts(&contacts)?;
    let mut res: HashMap<String, (usize, usize)> = HashMap::new();

    match requested_zone {
        Some(ref zone_name) => {
            if let Some(area_stats) = zones.get(zone_name) {
                for (area, (count, total_time)) in area_stats {
                    res.insert(area.clone(), (*count, total_time / count));
                }
            }
        },
        None => {
            for (zone_name, area_stats) in &zones {
                let (mut count, mut total) = (0, 0);
                for (_area_name, (c, t)) in area_stats {
                    count += c;
                    total += t;
                }
                res.insert(zone_name.clone(), (count, total / count));
            }
        }
    }

    Ok(res)
}

pub async fn get_unattempted(
    church_client: &mut ChurchClient,
    requested_zone: Option<String>,
) -> anyhow::Result<HashMap<String, usize>> {
    let persons_list = church_client.get_cached_people_list().await?.to_vec();
    let now = Utc::now().naive_utc();
    let persons_list: Vec<persons::Person> = persons_list
        .into_iter()
        .filter(|x| {
            x.referral_status == persons::ReferralStatus::NotAttempted
                && (x.person_status < persons::PersonStatus::NewMember)
                && now.signed_duration_since(x.assigned_date) < Duration::weeks(1)
        })
        .collect();
    
    let mut zones: HashMap<String, HashMap<String, usize>> = HashMap::new();

    for person in persons_list {
        if let Some(zone_name) = &person.zone_name {
            let zone = match zones.get_mut(zone_name) {
                Some(z) => z,
                None => {
                    zones.insert(zone_name.clone(), HashMap::new());
                    zones.get_mut(zone_name).unwrap()
                }
            };
             if let Some(area_name) = &person.area_name {
                let area = match zone.get_mut(area_name) {
                    Some(n) => n,
                    None => {
                        zone.insert(area_name.clone(), 0);
                        zone.get_mut(area_name).unwrap()
                    }
                };
             
                *area += 1;
            }
        }
    }
    let mut res: HashMap<String, usize> = HashMap::new();

    match requested_zone {
        Some(ref zone_name) => {
            if let Some(area_stats) = zones.get(zone_name) {
                for (area, count) in area_stats {
                    res.insert(area.clone(), count.clone());
                }
            }
        },
        None => {
            for (zone_name, area_stats) in &zones {
                let mut count = 0;
                for (_area_name, c) in area_stats {
                    count += c;
                }
                res.insert(zone_name.clone(), count);
            }
        }
    }      

    Ok(res)
}