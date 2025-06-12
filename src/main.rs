use chrono::Duration;
use church::ChurchClient;
use dialoguer::{theme::ColorfulTheme, Select};

use crate::holly::scheduled_times::RefetchPolicy;

mod bearer;
mod cache;
mod church;
mod env;
mod holly;
mod persons;
mod reports;

const CLI_OPTIONS: [&str; 5] = [
    "All Mission Weekly",
    "Zone Daily",
    "holly",
    "settings",
    "exit",
];
const CLI_DESCRIPTONS: [&str; 5] = [
    "All mission average response time, found referrals, and sacrament attendance",
    "Each Zone's average response time and uncontacted referrals",
    "Connects to Holly and responds to messages",
    "Change the settings for Holly",
    "Exits the program",
];

#[tokio::main]
async fn main() {
    println!("Starting referral list program... Checking environment...");
    let env = env::check_vars();
    env_logger::init();
    let mut church_client = church::ChurchClient::new(env).await.unwrap();

    let mut args = std::env::args();
    if args.len() > 1 {
        if let Err(e) = parse_argument(&args.nth(1).unwrap(), &mut church_client).await {
            println!("Ran into an error while processing: {e:?}");
        }
        return;
    }

    let select_options = CLI_OPTIONS
        .iter()
        .enumerate()
        .map(|(i, val)| format!("{} - {}", val, CLI_DESCRIPTONS[i]))
        .collect::<Vec<String>>();

    loop {
        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("Choose an option")
            .default(0)
            .items(&select_options)
            .interact()
            .unwrap();

        match parse_argument(CLI_OPTIONS[selection], &mut church_client).await {
            Ok(true) => continue,
            Ok(false) => return,
            Err(e) => {
                println!("Ran into an error while processing: {e:?}");
            }
        }
    }
}

async fn parse_argument(arg: &str, church_client: &mut ChurchClient) -> anyhow::Result<bool> {
    match arg {
        "All Mission Weekly" => {
            let env = church_client.env.clone();

            let report =
                reports::all_mission_report_weekly::AllMissionReportWeekly::generate_report(
                    church_client,
                    RefetchPolicy::RefetchAfter(Duration::hours(1)),
                )
                .await?;

            let output = report.pretty_print_report().await;
            report.save(&env)?;
            println!("{output}");
            Ok(true)
        }
        "Zone Daily" => {
            let env = church_client.env.clone();
            let holly_config = church_client.holly_config.clone().unwrap();

            let report = reports::zone_report_daily::ZoneReportDailyMap::generate_report(
                church_client,
                holly_config,
                RefetchPolicy::RefetchAfter(Duration::hours(1)),
            )
            .await?;
            let output = report.pretty_print_report(&env).await;
            report.save(&env)?;
            println!("{output}");
            Ok(true)
        }
        "holly" => {
            holly::main(church_client).await?;
            Ok(false)
        }
        "settings" => {
            let config = match holly::config::Config::potential_load(&church_client.env).await? {
                Some(mut c) => {
                    c.update(church_client).await?;
                    c
                }
                None => holly::config::Config::force_load(church_client).await?,
            };
            church_client.holly_config = Some(config);
            Ok(true)
        }
        "exit" => Ok(false),
        "help" | "-h" => {
            println!("Referral List - a tool to get and parse a list of referrals from referral manager.");
            for i in 0..CLI_OPTIONS.len() {
                println!("  {} - {}", CLI_OPTIONS[i], CLI_DESCRIPTONS[i]);
            }
            Ok(false)
        }
        _ => Err(anyhow::anyhow!(
            "Unknown usage '{arg}' - run without arguments to see options"
        )),
    }
}
