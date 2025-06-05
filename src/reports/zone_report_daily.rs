use crate::{
    church::ChurchClient,
    holly::{self, send_message::send_message},
    reports::{get_average, load_reports, save_report},
};
use std::collections::HashMap;

use log::{debug, info};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;

use super::{
    get_templates, get_unattempted, get_zone_name_from_id, pretty_print_avg_response_time,
};

pub type AreaAverageResponseTime = HashMap<String, (Vec<(String, String)>, usize)>;
pub type AreaUnattemptedCount = HashMap<String, Vec<(String, String)>>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ZoneReportDaily {
    pub area_avg_response_time: AreaAverageResponseTime,
    pub area_unattempted_count: AreaUnattemptedCount,
}

impl ZoneReportDaily {
    fn get_vars_for_template(&self) -> HashMap<String, String> {
        let mut result = HashMap::new();
        let area_avg_response_time =
            pretty_print_avg_response_time(self.area_avg_response_time.clone());
        let mut area_unattempted_count = String::new();

        let mut unattempted_entries: Vec<_> = self.area_unattempted_count.iter().collect();
        unattempted_entries.sort_by_key(|(area_name, _)| *area_name);

        for (area_name, people) in unattempted_entries {
            let count = people.len();
            area_unattempted_count.push_str(&format!("{area_name}: {count}\n"));
        }

        result.insert("area_avg_response_time".to_string(), area_avg_response_time);
        result.insert("area_unattempted_count".to_string(), area_unattempted_count);
        result
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ZoneReportDailyMap(pub HashMap<usize, ZoneReportDaily>);

impl ZoneReportDailyMap {
    pub async fn generate_report(
        church_client: &mut ChurchClient,
        holly_config: holly::config::Config,
    ) -> anyhow::Result<Self> {
        let report = Self::load(&church_client.env)?;
        if !report.0.is_empty() {
            return Ok(report);
        }

        let mut new_map = HashMap::new();

        for (zone_id, _) in holly_config.zone_chats {
            let zone_name = get_zone_name_from_id(&church_client.env, zone_id);
            let area_avg_response_time =
                get_average(church_client, zone_name.clone(), true, 1).await?;
            let area_unattempted_count = get_unattempted(church_client, zone_name.clone()).await?;

            new_map.insert(
                zone_id,
                ZoneReportDaily {
                    area_avg_response_time,
                    area_unattempted_count,
                },
            );
        }
        Ok(ZoneReportDailyMap(new_map))
    }

    fn load(env: &crate::env::Env) -> anyhow::Result<Self> {
        load_reports(env, "zone_report_daily.json")
    }

    pub fn save(&self, env: &crate::env::Env) -> anyhow::Result<()> {
        save_report(env, "zone_report_daily.json", self)
    }

    pub async fn send_report_to_holly(
        stream: &mut TcpStream,
        church_client: &mut ChurchClient,
        holly_config: holly::config::Config,
    ) -> anyhow::Result<()> {
        info!("Sending daily zone report to Holly...");
        let report = Self::generate_report(church_client, holly_config.clone()).await?;
        report.save(&church_client.env)?;
        
        let templates = get_templates(&church_client.env).await.unwrap();
        let template = templates
            .get("zone_report_daily")
            .ok_or_else(|| anyhow::anyhow!("Template 'zone_report_daily' not found"))?;
        for (zone_id, zone_report) in report.0.iter() {
            let chat_id = holly_config
                .zone_chats
                .get(zone_id)
                .ok_or_else(|| anyhow::anyhow!("Zone ID {zone_id} not found in holly config"))?;
            let vars = zone_report.get_vars_for_template();
            let message = template
                .replace(
                    "{area_avg_response_time}",
                    &vars
                        .get("area_avg_response_time")
                        .unwrap_or(&"".to_string()),
                )
                .replace(
                    "{area_unattempted_count}",
                    &vars
                        .get("area_unattempted_count")
                        .unwrap_or(&"".to_string()),
                );

            debug!("chat_id: {chat_id}, message: {message}");
            send_message(stream, message, chat_id.clone()).await?;
        }
        Ok(())
    }

    pub async fn pretty_print_report(&self, env: &crate::env::Env) -> String {
        let mut output = String::new();
        for (zone_id, zone_report) in self.0.iter() {
            if let Some(zone_name) = get_zone_name_from_id(env, *zone_id) {
                output.push_str(&format!("{zone_name}:\n"));
                let vars = zone_report.get_vars_for_template();

                let report: Vec<&str> = vars.values().clone().map(|s| s.as_str()).collect();

                output.push_str(&format!(
                    "Average Response time:\n{}\nUnattempted Contacted Referrals:\n{}\n",
                    report[0], report[1]
                ));
            }
        }
        output
    }
}

impl Default for ZoneReportDailyMap {
    fn default() -> Self {
        ZoneReportDailyMap(HashMap::new())
    }
}
