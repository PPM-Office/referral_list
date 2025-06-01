// Jackson Coxson

use std::collections::HashMap;

use chrono::{Duration, Utc};
use church::ChurchClient;
use dialoguer::{theme::ColorfulTheme, Select};
use indicatif::ProgressBar;
use log::info;
use log::debug;

mod bearer;
mod church;
mod env;
mod holly;
mod persons;
mod report;
mod reports;

const CLI_OPTIONS: [&str; 7] = ["report", "generate", "average", "zone_report", "holly", "settings", "exit"];
const CLI_DESCRIPTONS: [&str; 7] = [
    "Reads today's report of uncontacted referrals or fetches a new one",
    "Generates a new list of uncontacted referrals, regardless of the cache.",
    "Gets the average contact time in minutes between zones",
    "Generates zone average",
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
        "report" => {
            if let Some(report) = report::Report::read_report(&church_client.env)? {
                println!("{}", report.pretty_print());
            } else {
                let report = generate_report(church_client).await?;
                println!("{}", report.pretty_print());
            }
            Ok(true)
        }
        "generate" => {
            generate_report(church_client).await?;
            Ok(true)
        }
        "average" => {
            // let contacts = reports::get_average(church_client, None, false).await?;

            // let output = pretty_print_average_areas(contacts.data);
            Ok(true)
        }
        "zone_report" => {
            let env = church_client.env.clone();
            let holly_config = church_client.holly_config.clone().unwrap();

            let report = reports::zone_report_daily::ZoneReportDailyMap::generate_report(church_client, holly_config).await?;
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

pub async fn generate_report(church_client: &mut ChurchClient) -> anyhow::Result<report::Report> {
    let persons_list = church_client.get_cached_people_list().await?;
    let now = Utc::now().naive_utc();
    let persons_list: Vec<persons::Person> = persons_list
        .into_iter()
        .filter(|x| {
            (x.referral_status != persons::ReferralStatus::Successful
                && x.person_status < persons::PersonStatus::NewMember
                && now.signed_duration_since(x.assigned_date) > Duration::hours(48))
            || x.referral_status == persons::ReferralStatus::NotAttempted
        })
        .collect();
    info!("{} uncontacted referrals", persons_list.len());

    let mut report = report::Report::new();
    let bar = ProgressBar::new(persons_list.len() as u64);
    for person in persons_list {
        bar.inc(1);
        if match church_client.get_person_last_contact(&person).await? {
            Some(t) => now.signed_duration_since(t) > Duration::hours(48),
            None => true,
        } {
            report.add_person(person);
        }
    }

    report.save_report(&church_client.env)?;
    Ok(report)
}

pub fn pretty_print_average_areas(data: HashMap<String, (usize, usize)>) -> String {
    let mut res = "".to_string();
    for (area, (count,avg)) in data {
        let hours = avg / 60;
        let minutes = avg % 60;
        res = format!("{res}\n{area}: ({count}) {hours}h {minutes}m");
    }
    res
}