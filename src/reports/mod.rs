use std::{collections::HashMap, fs, path::PathBuf};

use chrono::{DateTime, Local};
use indicatif::ProgressBar;
use log::info;
use serde::{de::DeserializeOwned, Serialize};
use zone_report_daily::{AreaAverageResponseTime, AreaUnattemptedCount};

use crate::persons::{self, Person};

pub mod all_mission_report_weekly;
pub mod zone_report_daily;
pub mod zone_report_nightly;

pub fn load_reports<T: DeserializeOwned + Default>(
    env: &crate::env::Env,
    date: Option<DateTime<Local>>,
    filename: &str,
) -> anyhow::Result<T> {
    let date = match date {
        Some(date) => date,
        None => chrono::Local::now(),
    };

    let date_str = date.format("%Y-%m-%d").to_string();
    let mut dir_path = PathBuf::from(&env.working_path);
    dir_path.push("reports");
    dir_path.push(&date_str);

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
        return Err(anyhow::anyhow!(
            "Templates file not found at {:?}",
            file_path
        ));
    }

    let file = std::fs::File::open(file_path)?;
    let templates = serde_json::from_reader(file).unwrap();

    Ok(templates)
}
pub async fn get_average(
    requested_zone: Option<String>,
    persons_list: Vec<Person>,
) -> anyhow::Result<AreaAverageResponseTime> {
    let mut zones = aggregate_zones_areas(persons_list.clone(), |_person| {
        (Vec::<(String, String)>::new(), 0usize)
    });

    info!("Caclulating average response time");
    let bar = ProgressBar::new(persons_list.len() as u64);
    for person in persons_list {
        bar.inc(1);
        if person.response_time.is_none()
            || person.zone_name.is_none()
            || person.area_name.is_none()
        {
            continue;
        }

        let response_time = person.response_time.unwrap();
        let zone_name = person.zone_name.as_ref().unwrap();
        let area_name = person.area_name.as_ref().unwrap();

        if let Some(zone) = zones.get_mut(zone_name) {
            if let Some(area) = zone.get_mut(area_name) {
                area.0
                    .push((person.guid.clone(), person.first_name.clone()));
                area.1 += response_time;
            }
        }
    }

    let mut res: AreaAverageResponseTime = HashMap::new();

    match requested_zone {
        Some(ref zone_name) => {
            if let Some(area_stats) = zones.get(zone_name) {
                let mut zone_total_people: Vec<(String, String)> = Vec::new();
                let mut zone_total_time = 0;
                for (area, (people, total_time)) in area_stats {
                    let count = people.len();
                    zone_total_people.extend(people.iter().cloned());
                    zone_total_time += total_time;
                    let avg = if zone_total_time > 0 {
                        zone_total_time / count
                    } else {
                        0
                    };

                    res.insert(area.clone(), (people.to_vec(), avg));
                }
                res.insert(
                    "Zone Total".to_string(),
                    (
                        zone_total_people.clone(),
                        zone_total_time / zone_total_people.len(),
                    ),
                );
            }
        }
        None => {
            let mut total_people: Vec<(String, String)> = Vec::new();
            let mut total_time = 0;
            for (zone_name, area_stats) in &zones {
                let (mut people, mut total) = (Vec::new(), 0);
                for (_area_name, (p, t)) in area_stats {
                    people.extend(p.iter().cloned());
                    total += t;
                }
                total_people.extend(people.iter().cloned());
                total_time += total;
                let count = people.len();
                res.insert(zone_name.clone(), (people, total / count));
            }
            res.insert(
                "Mission Total".to_string(),
                (total_people.clone(), total_time / total_people.len()),
            );
        }
    }

    bar.finish();
    Ok(res)
}

pub fn pretty_print_avg_response_time(
    avg_response_time: HashMap<String, (Vec<(String, String)>, usize)>,
) -> String {
    let mut output = String::new();
    let mut avg_entries: Vec<_> = avg_response_time.iter().collect();
    avg_entries.sort_by(|(a, _), (b, _)| {
        let a_is_total = a.contains("Total");
        let b_is_total = b.contains("Total");
        match (a_is_total, b_is_total) {
            (true, false) => std::cmp::Ordering::Less, // a comes first
            (false, true) => std::cmp::Ordering::Greater, // b comes first
            _ => a.cmp(b),                             // alphabetical
        }
    });

    for (area_name, (people, average)) in avg_entries {
        let hours = average / 60;
        let minutes = average % 60;
        let count = people.len();
        output.push_str(&format!(
            "{area_name}:\nReceived: {count}, Avg: {hours}h {minutes}m\n\n"
        ));
    }
    return output;
}

pub async fn get_uncontacted(
    requested_zone: Option<String>,
    persons_list: Vec<Person>,
) -> anyhow::Result<AreaUnattemptedCount> {
    let mut zones = aggregate_zones_areas(persons_list.clone(), |_person| {
        Vec::<(String, String)>::new()
    });

    for person in persons_list {
        if let (Some(zone_name), Some(area_name)) = (&person.zone_name, &person.area_name) {
            if let Some(zone) = zones.get_mut(zone_name) {
                if let Some(area) = zone.get_mut(area_name) {
                    area.push((person.guid.clone(), person.first_name.clone()));
                }
            }
        }
    }

    let mut res: AreaUnattemptedCount = HashMap::new();

    match requested_zone {
        Some(ref zone_name) => {
            if let Some(area_stats) = zones.get(zone_name) {
                for (area, people) in area_stats {
                    res.insert(area.clone(), people.to_vec());
                }
            }
        }
        None => {
            for (zone_name, area_stats) in &zones {
                let mut people = Vec::new();
                for (_area_name, p) in area_stats {
                    people.extend(p.iter().cloned());
                }
                res.insert(zone_name.clone(), people);
            }
        }
    }

    Ok(res)
}

fn aggregate_zones_areas<T, F>(
    persons_list: Vec<persons::Person>,
    mut value_fn: F,
) -> HashMap<String, HashMap<String, T>>
where
    F: FnMut(&persons::Person) -> T,
{
    let mut zones: HashMap<String, HashMap<String, T>> = HashMap::new();
    for person in persons_list {
        if let (Some(zone_name), Some(area_name)) = (&person.zone_name, &person.area_name) {
            let zone = zones.entry(zone_name.clone()).or_insert_with(HashMap::new);
            zone.entry(area_name.clone())
                .or_insert_with(|| value_fn(&person));
            // For Vec types, you may want to push instead of overwrite; see usage below
        }
    }
    zones
}
