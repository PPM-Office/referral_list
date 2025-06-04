use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{church::ChurchClient, holly};

use super::{get_average, load_reports, pretty_print_avg_response_time, save_report};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AllMissionReportWeekly {
    pub zone_avg_response_time: HashMap<String, (Vec<(String, String)>, usize)>,
    pub percent_referrals_taught: f32,
    pub numb_of_referrals_at_sacrament: usize,
}

impl AllMissionReportWeekly {
    pub async fn generate_report(
        church_client: &mut ChurchClient,
        holly_config: holly::config::Config,
    ) -> anyhow::Result<Self> {
        let report = Self::load(&church_client.env)?;
        if !report.zone_avg_response_time.is_empty() {
            return Ok(report);
        }

        let zone_avg_response_time = get_average(church_client, None, false, 7).await?;

        Ok(AllMissionReportWeekly {
            zone_avg_response_time,
            ..Default::default()
        })
    }

    fn load(env: &crate::env::Env) -> anyhow::Result<Self> {
        load_reports(env, "all_mission_report_weekly.json")
    }

    pub fn save(&self, env: &crate::env::Env) -> anyhow::Result<()> {
        save_report(env, "all_mission_report_weekly.json", self)
    }

    pub async fn pretty_print_report(&self, env: &crate::env::Env) -> String {
        let zone_average = pretty_print_avg_response_time(self.zone_avg_response_time.clone());

        zone_average
    }
}

impl Default for AllMissionReportWeekly {
    fn default() -> Self {
        AllMissionReportWeekly {
            zone_avg_response_time: HashMap::new(),
            percent_referrals_taught: 0 as f32,
            numb_of_referrals_at_sacrament: 0 as usize,
        }
    }
}
