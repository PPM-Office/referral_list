// Jackson Coxson
// Code to interact with church servers

use std::{
    collections::{HashMap, HashSet},
    io::Write,
    path::PathBuf,
    str::FromStr,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use chrono::{Local, NaiveDateTime, NaiveTime, Utc};
use futures::future::join_all;
use indicatif::ProgressBar;
use log::{debug, info, warn};
use reqwest::{redirect::Policy, Client};
use reqwest_cookie_store::CookieStoreMutex;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::Mutex;

use crate::{
    bearer::BearerToken,
    cache::PersonsCacheMap,
    env,
    holly::scheduled_times::RefetchPolicy,
    persons::{self, Person, TimelineContactType, TimelineEvent, TimelineItemType},
};

pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36";
const MAX_RETRIES: u8 = 3;

#[derive(Debug, Clone)]
pub struct ChurchClient {
    http_client: Client,
    cookie_store: Arc<CookieStoreMutex>,
    pub env: env::Env,
    bearer_token: Option<BearerToken>,
    pub holly_config: Option<crate::holly::config::Config>,
}

impl ChurchClient {
    pub async fn new(env: env::Env) -> anyhow::Result<Self> {
        // Check if the bearer token exists
        let bearer_path = PathBuf::from_str(&env.working_path)?.join("bearer.token");
        let cookies_path = PathBuf::from_str(&env.working_path)?.join("cookies.json");

        let bearer_token = if let Ok(b) = std::fs::read_to_string(&bearer_path) {
            Some(BearerToken::from_base64(b)?)
        } else {
            info!("No bearer token saved");
            None
        };

        // Check if the file exists
        if !std::fs::exists(&cookies_path)? {
            info!("No cookies saved");
            std::fs::write(&cookies_path, "".as_bytes())?;
        }
        let cookie_store = {
            let file = std::fs::File::open(&cookies_path)
                .map(std::io::BufReader::new)
                .unwrap();
            // use re-exported version of `CookieStore` for crate compatibility
            reqwest_cookie_store::CookieStore::load_json(file).unwrap()
        };
        let cookie_store = reqwest_cookie_store::CookieStoreMutex::new(cookie_store);
        let cookie_store = std::sync::Arc::new(cookie_store);

        let http_client = Client::builder()
            .user_agent(USER_AGENT)
            .cookie_provider(Arc::clone(&cookie_store))
            .redirect(Policy::custom(|a| {
                if a.previous().len() > 2 {
                    a.stop()
                } else {
                    info!("Redirecting to {}", a.url());
                    a.follow()
                }
            }))
            .timeout(std::time::Duration::from_secs(240))
            .build()
            .expect("Couldn't build the HTTP client");

        let holly_config = crate::holly::config::Config::potential_load(&env).await?;

        Ok(Self {
            http_client,
            cookie_store,
            env,
            bearer_token,
            holly_config,
        })
    }

    pub async fn save_cookies(&self) -> anyhow::Result<()> {
        info!("Saving cookies");
        let cookies_path = PathBuf::from_str(&self.env.working_path)?.join("cookies.json");
        let mut writer = std::fs::File::create(&cookies_path)
            .map(std::io::BufWriter::new)
            .unwrap();
        let store = self.cookie_store.lock().unwrap();
        store
            .save_incl_expired_and_nonpersistent_json(&mut writer)
            .unwrap();
        Ok(())
    }

    async fn write_bearer_token(&self, token: &str) -> anyhow::Result<()> {
        info!("Saving bearer token");
        let bearer_path = PathBuf::from_str(&self.env.working_path)?.join("bearer.token");
        let mut writer = std::fs::File::create(&bearer_path)
            .map(std::io::BufWriter::new)
            .unwrap();
        writer.write_all(token.as_bytes())?;
        Ok(())
    }

    /// Logs into churchofjesuschrist.org
    pub async fn login(&mut self) -> anyhow::Result<BearerToken> {
        info!("Logging into referral manager");
        self.cookie_store.lock().unwrap().clear();

        // Get the inital login page
        info!("Loading the initial login page");
        let res = self
            .http_client
            .get("https://referralmanager.churchofjesuschrist.org")
            .send()
            .await?
            .text()
            .await?;

        // Extract the JSON embedded in the HTML
        let start_token = "\"stateToken\":\"";
        let end_token = "\",";

        let start_index = res
            .find(start_token)
            .ok_or_else(|| anyhow::anyhow!("stateToken not found in response"))?
            + start_token.len();

        let end_index = res[start_index..]
            .find(end_token)
            .ok_or_else(|| anyhow::anyhow!("End token not found in response"))?
            + start_index;

        // Ensure the indices are valid
        if start_index >= end_index {
            return Err(anyhow::anyhow!("Invalid indices for stateToken extraction"));
        }

        let state_token = &res[start_index..end_index];
        let state_token = decode_escape_sequences(state_token)?;
        let state_token: String = serde_json::from_str(&format!("\"{state_token}\"")).unwrap();

        #[derive(Deserialize)]
        struct StateHandle {
            #[serde(rename = "stateHandle")]
            state_handle: String,
        }
        // Trade the state token for the state handle
        info!("Trading the token for the state handle");
        let state_handle = self
            .http_client
            .post("https://id.churchofjesuschrist.org/idp/idx/introspect")
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .body(format!("{{\"stateToken\": \"{state_token}\"}}"))
            .send()
            .await?
            .json::<StateHandle>()
            .await?
            .state_handle;

        // Send the username
        info!("Sending the username");
        let body = json!({
            "stateHandle": state_handle,
            "identifier": self.env.church_username
        })
        .to_string();
        let state_handle = self
            .http_client
            .post("https://id.churchofjesuschrist.org/idp/idx/identify")
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .body(body)
            .send()
            .await?
            .json::<StateHandle>()
            .await?
            .state_handle;

        // Send the password
        #[derive(Deserialize)]
        struct PasswordResponse {
            success: SuccessResponse,
        }

        #[derive(Deserialize)]
        struct SuccessResponse {
            href: String,
        }

        info!("Sending the password");
        let body = json!({
            "stateHandle": state_handle,
            "credentials": {
                "passcode": self.env.church_password
            }
        })
        .to_string();
        let res = self
            .http_client
            .post("https://id.churchofjesuschrist.org/idp/idx/challenge/answer")
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .body(body)
            .send()
            .await?
            .json::<PasswordResponse>()
            .await?;

        // Set cookies
        info!("Getting the success href");
        self.http_client.get(res.success.href).send().await?;

        // Get the bearer token
        info!("Getting the bearer token");
        let token = self
            .http_client
            .get("https://referralmanager.churchofjesuschrist.org/services/auth")
            .header("Accept", "application/json")
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?["token"]
            .clone();
        let token = match token {
            serde_json::Value::String(t) => Ok(t),
            _ => Err(anyhow::anyhow!("No token in response json")),
        }?;

        self.save_cookies().await?;
        self.write_bearer_token(&token).await?;

        let token = BearerToken::from_base64(token)?;
        self.bearer_token = Some(token.clone());

        Ok(token)
    }

    /// Gets the list of everyone from the referral manager. This is a HUGE request at roughly 8mb in the CSDM
    pub async fn get_people_list(&mut self) -> anyhow::Result<Vec<persons::Person>> {
        info!("Getting the people list from referral manager");
        let mut tries = 0;

        while tries < MAX_RETRIES {
            let token = match &self.bearer_token {
                Some(t) => t,
                None => &self.login().await?,
            };
            tries += 1;
            if let Ok(list) = self.http_client.get(format!("https://referralmanager.churchofjesuschrist.org/services/people/mission/{}?includeDroppedPersons=true", token.claims.mission_id))
            .header("Authorization", format!("Bearer {}", token.token))
            .send().await {
                if let Ok(list) = list.json::<serde_json::Value>().await {
                    let list = persons::Person::parse_lossy(list);
                    info!("Received {} people from referral manager", list.len());
                    return Ok(list);
                } else {
                    warn!("Getting the people list failed at JSON parse");
                    self.bearer_token = None;
                }
            } else {
                warn!("Getting the people list failed at the request");
                self.bearer_token = None;
            }
        }
        Err(anyhow::anyhow!("Max tries exceeded"))
    }

    /// Gets a cached list from referral manager to save trips to church servers.
    /// A cache will be considered 'hit' if the list is less than an hour old.
    pub async fn get_cached_people_list<F>(
        &mut self,
        filter: F,
        refetch_policy: RefetchPolicy,
        apply_timeline_cache: bool,
    ) -> anyhow::Result<Vec<Person>>
    where
        F: Fn(&Person) -> bool,
    {
        let lists_path = PathBuf::from_str(&self.env.working_path)?.join("people_lists");
        std::fs::create_dir_all(&lists_path)?;

        let now = SystemTime::now();
        let now = now
            .duration_since(UNIX_EPOCH)
            .context("Your clock is wrong")?
            .as_secs();

        let mut latest_time: u64 = 0;

        for entry in std::fs::read_dir(&lists_path)? {
            if let Some(timestamp) = extract_valid_file_timestamp(&entry, now) {
                if latest_time < timestamp {
                    latest_time = timestamp;
                }
                if let RefetchPolicy::RefetchAfter(duration) = refetch_policy {
                    let diff = now.checked_sub(timestamp);
                    if diff < Some(duration.num_seconds() as u64) {
                        info!("Cache hit!");
                        let lossy_person_list: Vec<Person> = persons::Person::parse_lossy(
                            serde_json::from_str(&std::fs::read_to_string(entry?.path()).unwrap())?,
                        )
                        .into_iter()
                        .filter(|p| filter(p))
                        .collect();

                        let _ = self.update_zone_and_area_ids(lossy_person_list.clone())?;

                        if apply_timeline_cache {
                            let list_with_timeline_cache = self
                                .update_list_with_timeline_cache(lossy_person_list, refetch_policy)
                                .await?;
                            return Ok(list_with_timeline_cache);
                        }

                        return Ok(lossy_person_list);
                    }
                }
            }
        }

        if refetch_policy == RefetchPolicy::NoRefetch && latest_time > 0 {
            info!("Using old cache");
            let file_path = lists_path.join(format!("{latest_time}.json"));
            let lossy_person_list: Vec<Person> = persons::Person::parse_lossy(
                serde_json::from_str(&std::fs::read_to_string(file_path).unwrap())?,
            )
            .into_iter()
            .filter(|p| filter(p))
            .collect();

            let _ = self.update_zone_and_area_ids(lossy_person_list.clone())?;

            if apply_timeline_cache {
                let list_with_timeline_cache = self
                    .update_list_with_timeline_cache(lossy_person_list, refetch_policy)
                    .await?;

                return Ok(list_with_timeline_cache);
            }

            return Ok(lossy_person_list);
        }

        info!("Cache missed");
        let mut list = self
            .get_people_list()
            .await?
            .into_iter()
            .filter(|p| filter(p))
            .collect();

        if apply_timeline_cache {
            list = self
                .update_list_with_timeline_cache(list, refetch_policy)
                .await?;
        }

        let file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(lists_path.join(format!("{now}.json")))?;

        serde_json::to_writer(file, &json!({"persons": &list}))?;

        let _ = self.update_zone_and_area_ids(list.clone())?;

        Ok(list)
    }

    fn update_zone_and_area_ids(&self, people_list: Vec<Person>) -> anyhow::Result<()> {
        let zone_id_path = PathBuf::from_str(&self.env.working_path)?.join("zone_ids.json");
        let area_id_oath = PathBuf::from_str(&self.env.working_path)?.join("area_ids.json");

        let mut zone_ids = HashSet::new();
        let mut zones = Vec::new();
        let mut area_ids = HashSet::new();
        let mut areas = Vec::new();

        for person in people_list {
            if let (Some(zone_name), Some(zone_id)) = (person.zone_name, person.zone_id) {
                if zone_ids.insert(zone_id) {
                    zones.push((zone_id, zone_name));
                }
            }
            if let (Some(area_name), Some(area_id)) = (person.area_name, person.area_id) {
                if area_ids.insert(area_id) {
                    areas.push((area_id, area_name));
                }
            }
        }

        let zone_ids_file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(zone_id_path)?;

        serde_json::to_writer(zone_ids_file, &json!(zones))?;

        let area_ids_file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(area_id_oath)?;

        serde_json::to_writer(area_ids_file, &json!(areas))?;

        Ok(())
    }

    pub async fn get_person_timeline(
        &mut self,
        person: &persons::Person,
    ) -> anyhow::Result<Vec<persons::TimelineEvent>> {
        let mut tries = 0;

        while tries < MAX_RETRIES {
            tries += 1;
            if let Ok(list) = self
                .http_client
                .get(format!(
                    "https://referralmanager.churchofjesuschrist.org/services/progress/timeline/{}",
                    person.guid
                ))
                .send()
                .await
            {
                if let Ok(list) = list.json::<serde_json::Value>().await {
                    let list = persons::TimelineEvent::parse_lossy(list);
                    debug!(
                        "Received {} timeline events from referral manager",
                        list.len()
                    );
                    return Ok(list);
                } else {
                    warn!("Getting the timeline events list failed at JSON parse");
                    self.login().await?;
                }
            } else {
                warn!("Getting the timeline events list failed at the request");
                self.login().await?;
            }
        }
        Err(anyhow::anyhow!("Max tries exceeded"))
    }

    fn utc_naive_to_local_time(naive_utc: NaiveDateTime) -> NaiveDateTime {
        let utc_dt = chrono::DateTime::<Utc>::from_naive_utc_and_offset(naive_utc, Utc);
        let local_dt = utc_dt.with_timezone(&Local);
        local_dt.naive_local()
    }

    pub async fn get_person_contact_time(
        &mut self,
        timeline: &Vec<persons::TimelineEvent>,
    ) -> anyhow::Result<Option<usize>> {
        let mut referral_sent = None;
        let mut last_contact = None;
        for item in timeline {
            match item.item_type {
                persons::TimelineItemType::NewReferral => {
                    referral_sent = Some(item.item_date);
                    last_contact = None;
                }
                persons::TimelineItemType::Contact | persons::TimelineItemType::Teaching => {
                    if item.status.is_some() && last_contact.is_none() {
                        last_contact = Some(item.item_date);
                    }
                }
                _ => {
                    continue;
                }
            }
        }
        Ok(Self::get_adjusted_contact_time(referral_sent, last_contact))
    }

    fn get_adjusted_contact_time(
        referral_sent: Option<NaiveDateTime>,
        last_contact: Option<NaiveDateTime>,
    ) -> Option<usize> {
        if let Some(referral_sent) = referral_sent {
            if let Some(last_contact) = last_contact {
                let referral_sent = Self::utc_naive_to_local_time(referral_sent);
                let last_contact = Self::utc_naive_to_local_time(last_contact);

                // TODO: move to a config file
                let start_of_day = NaiveTime::from_hms_opt(10, 00, 0).unwrap();
                let end_of_day = NaiveTime::from_hms_opt(22, 15, 0).unwrap();

                let mut total_minutes = 0;

                // If referral_sent and last_contact are on the same day
                if referral_sent.date() == last_contact.date() {
                    let from = referral_sent.time().max(start_of_day);
                    let to = last_contact.time().min(end_of_day);
                    if to > from {
                        total_minutes = (to - from).num_minutes() as usize;
                    }
                } else {
                    // Minutes from referral_sent to end_of_day
                    let from = referral_sent.time().max(start_of_day);
                    let to = end_of_day;
                    if to > from {
                        total_minutes += (to - from).num_minutes() as usize;
                    }

                    // Minutes from start_of_day to last_contact on the last day
                    let from = start_of_day;
                    let to = last_contact.time().min(end_of_day);
                    if to > from {
                        total_minutes += (to - from).num_minutes() as usize;
                    }

                    // Add full days in between, if any
                    let days_between = (last_contact.date() - referral_sent.date()).num_days() - 1;
                    if days_between > 0 {
                        total_minutes += days_between as usize
                            * (end_of_day - start_of_day).num_minutes() as usize;
                    }
                }

                return Some(total_minutes);
            }
        }
        None
    }

    pub async fn get_timeline_stats(
        &mut self,
        timeline: &Vec<TimelineEvent>,
    ) -> (
        Option<usize>,
        bool,
        bool,
        usize,
        bool,
        bool,
        bool,
        Option<usize>,
    ) {
        let response_time = match self.get_person_contact_time(timeline).await {
            Ok(response_time) => response_time,
            Err(_) => None,
        };
        let mut has_attended_sacrament: bool = false;
        let mut has_attended_since_last_referral: bool = false;
        let mut sacrament_attendance_count: usize = 0;
        let mut has_been_taught: bool = false;
        let mut has_been_taught_in_person: bool = false;
        let mut has_been_taught_since_last_referral: bool = false;
        let mut referral_count: Option<usize> = None;

        for event in timeline {
            if event.item_type == TimelineItemType::Sacrament {
                sacrament_attendance_count += 1;
                has_attended_sacrament = true;
                has_attended_since_last_referral = true;
            }
            if let Some(status) = event.status {
                if event.item_type == TimelineItemType::Teaching && status {
                    has_been_taught = true;
                    if let Some(contact_type) = &event.contact_type {
                        if contact_type == &TimelineContactType::InPerson {
                            has_been_taught_in_person = true;
                        }
                    }
                }
            }

            if event.item_type == TimelineItemType::NewReferral {
                referral_count = match referral_count {
                    Some(count) => Some(count + 1),
                    None => Some(1),
                };
                has_attended_since_last_referral = false;
                has_been_taught_since_last_referral = false;
            }
        }

        (
            response_time,
            has_attended_sacrament,
            has_attended_since_last_referral,
            sacrament_attendance_count,
            has_been_taught,
            has_been_taught_in_person,
            has_been_taught_since_last_referral,
            referral_count,
        )
    }

    async fn update_list_with_timeline_cache(
        &self,
        persons_list: Vec<Person>,
        refetch_policy: RefetchPolicy,
    ) -> anyhow::Result<Vec<Person>> {
        let initial_cache_map = PersonsCacheMap::get_cache(self.env.clone());
        let initial_cache_map = match initial_cache_map {
            Ok(r) => r,
            Err(_) => PersonsCacheMap(HashMap::new()),
        };
        let cache_map = Arc::new(Mutex::new(initial_cache_map));
        let client = Arc::new(Mutex::new(self.clone())); // clone self once
        let mut tasks = vec![];

        let persons_arc: Vec<_> = persons_list
            .into_iter()
            .map(|p| Arc::new(Mutex::new(p)))
            .collect();

        info!("Applying timeline cache");
        let bar = Arc::new(Mutex::new(ProgressBar::new(persons_arc.len() as u64)));

        for person_arc in &persons_arc {
            let person = Arc::clone(&person_arc);
            let cache_map = Arc::clone(&cache_map);
            let client = Arc::clone(&client);
            let bar = Arc::clone(&bar);

            let task = tokio::spawn(async move {
                let mut person_guard = person.lock().await;
                let mut cache_lock = cache_map.lock().await;
                let mut client_lock = client.lock().await;
                let bar_lock = bar.lock().await;

                person_guard
                    .apply_cache(&mut client_lock, &mut cache_lock, refetch_policy)
                    .await;

                bar_lock.inc(1);

                person_guard.clone()
            });

            tasks.push(task);
        }
        let updated_list: Vec<Person> = join_all(tasks)
            .await
            .into_iter()
            .filter_map(|res| res.ok()) // filter out any task failures
            .collect();

        Arc::try_unwrap(bar)
            .expect("Bar references failed to drop")
            .into_inner()
            .finish();

        let cache_guard = Arc::try_unwrap(cache_map)
            .expect("All references should be dropped")
            .into_inner();

        let _ = cache_guard.save_cache(self.clone().env);

        return Ok(updated_list);
    }
}

fn extract_valid_file_timestamp(
    entry: &std::io::Result<std::fs::DirEntry>,
    now: u64,
) -> Option<u64> {
    let entry = entry.as_ref().ok()?;

    if !entry.file_type().ok()?.is_file() {
        return None;
    }

    let file_name = entry.file_name().into_string().ok()?;
    let (timestamp_str, _) = file_name.split_once('.')?;
    let timestamp = timestamp_str.parse::<u64>().ok()?;

    Some(timestamp)
}

/// Function to decode escape sequences including \xNN
fn decode_escape_sequences(s: &str) -> anyhow::Result<String> {
    // Replace URL encoded sequences
    let decoded_string = s
        .replace("\\x2D", "-") // Replace \x2D with '-'
        .replace("\\x5F", "_") // Replace \x5F with '_'
        .replace("\\x2E", ".") // Replace \x2E with '.'
        .replace("\\x2F", "/") // Replace \x2F with '/'
        .replace("\\x3D", "="); // Replace \x3D with '='

    Ok(decoded_string.to_string())
}
