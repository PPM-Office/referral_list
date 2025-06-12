use std::collections::HashMap;

use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    church::ChurchClient,
    holly::scheduled_times::RefetchPolicy,
    persons::{self, Person, PersonStatus},
};

use super::{get_average, load_reports, pretty_print_avg_response_time, save_report};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AllMissionReportWeekly {
    pub zone_avg_response_time: HashMap<String, (Vec<(String, String)>, usize)>,
    pub referrals_found: Vec<(String, String)>,
    pub referrals_received_count: usize,
    pub referrals_at_church: Vec<(String, String)>,
}

impl AllMissionReportWeekly {
    pub async fn generate_report(
        church_client: &mut ChurchClient,
        refetch_policy: RefetchPolicy,
    ) -> anyhow::Result<Self> {
        let report = Self::load(&church_client.env)?;
        if !report.zone_avg_response_time.is_empty() {
            return Ok(report);
        }

        let now = Utc::now().naive_utc();

        let full_list_filter = |person: &Person| -> bool {
            person.referral_status != persons::ReferralStatus::NotAttempted
                && (person.person_status < persons::PersonStatus::NewMember)
                && now.signed_duration_since(person.assigned_date) < Duration::days(7)
        };

        let green_list_filter = |person: &Person| -> bool {
            return full_list_filter(person) && person.person_status != PersonStatus::Yellow;
        };

        // force updated stats for ONLY the people FOUND in past week.
        // if we did it for every referral in past week, we'd hit the church server 1000+ times
        // which probably isn't chill
        let green_people_list = church_client
            .get_cached_people_list(green_list_filter, refetch_policy, true)
            .await?;

        // lazy grab full list. We'll only grab what's already been cached
        let full_people_list = church_client
            .get_cached_people_list(full_list_filter, RefetchPolicy::NoRefetch, true)
            .await?;

        let zone_avg_response_time = get_average(None, full_people_list.clone()).await?;
        let referrals_found = get_referrals_found(green_people_list.clone());
        let referrals_at_church = get_referrals_at_church_count(green_people_list.clone());

        Ok(AllMissionReportWeekly {
            zone_avg_response_time,
            referrals_found,
            referrals_received_count: full_people_list.len(),
            referrals_at_church,
            ..Default::default()
        })
    }

    fn load(env: &crate::env::Env) -> anyhow::Result<Self> {
        load_reports(env, None, "all_mission_report_weekly.json")
    }

    pub fn save(&self, env: &crate::env::Env) -> anyhow::Result<()> {
        save_report(env, "all_mission_report_weekly.json", self)
    }

    pub async fn pretty_print_report(&self) -> String {
        let zone_average = pretty_print_avg_response_time(self.zone_avg_response_time.clone());
        let percentage = if self.referrals_received_count != 0 {
            format!(
                "{:.2}%",
                (self.referrals_found.len() as f64 / self.referrals_received_count as f64) * 100.0
            )
        } else {
            "N/A".to_string()
        };

        let output = format!("Average Response Time:\n{zone_average}\n\nPercent Referrals Taught: {}/{} ({})\nReferrals at Sacrament Meeting: {}", self.referrals_found.len(), self.referrals_received_count, percentage, self.referrals_at_church.len());
        output
    }
}

pub fn get_referrals_found(people_list: Vec<Person>) -> Vec<(String, String)> {
    let referrals_found: Vec<(String, String)> = people_list
        .into_iter()
        .filter(|person| {
            person.person_status < PersonStatus::NewMember
                && person.person_status != PersonStatus::Yellow
        })
        .map(|person| (person.guid, person.first_name))
        .collect();

    referrals_found
}

pub fn get_referrals_at_church_count(people_list: Vec<Person>) -> Vec<(String, String)> {
    return people_list
        .into_iter()
        .filter(|person| matches!(person.has_attended_since_last_referral, Some(true)))
        .map(|person| (person.guid, person.first_name))
        .collect();
}

impl Default for AllMissionReportWeekly {
    fn default() -> Self {
        AllMissionReportWeekly {
            zone_avg_response_time: HashMap::new(),
            referrals_found: Vec::new(),
            referrals_received_count: 0,
            referrals_at_church: Vec::new(),
        }
    }
}
