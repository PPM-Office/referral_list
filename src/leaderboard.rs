use std::collections::HashMap;

use indicatif::ProgressBar;

use crate::{church::ChurchClient, missionary::{Missionary, ProsArea}, persons};

pub struct LeaderBoard {
    items: Vec<LeaderBoardItem>
}

struct LeaderBoardItem {
    area_name: String,
    missionaries: Vec<Missionary>,
    response_time: usize
}

impl LeaderBoardItem {
    fn new(missionaries: Vec<Missionary>, area_name: String, response_time: usize) -> Self {
        LeaderBoardItem {
            area_name,
            missionaries,
            response_time
        }
    }
}

impl LeaderBoard {
    pub async fn generate_leaderboard(church_client: &mut ChurchClient, top_nth: usize, incrementing: bool) -> anyhow::Result<LeaderBoard> {
        let mut contacts = church_client.env.load_contacts()?;
        let persons_list = persons::Person::get_person_list(church_client).await?;
        
        let mut pros_areas: HashMap<usize, Vec<usize>> = HashMap::new();
        let bar = ProgressBar::new(persons_list.len() as u64);
        for person in persons_list {
            if let Some(pros_area_id) = person.area_id {
                bar.inc(1);
                let time = if let Some(time) = contacts.get(&person.guid) {
                    time.to_owned()
                } else if let Some(time) = church_client.get_person_contact_time(&person).await? {
                    contacts.insert(person.guid, time);
                    time
                } else {
                    continue;
                };

                let area = match pros_areas.get_mut(&pros_area_id) {
                    Some(a) => a,
                    None => {
                        pros_areas.insert(pros_area_id, Vec::new());
                        pros_areas.get_mut(&pros_area_id).unwrap()
                    }
                };

                area.push(time);                
            }
        }
        bar.finish();

        
        church_client.env.save_contacts(&contacts)?;
        let mut leaderboard = HashMap::new();
        for (area_id, times) in pros_areas {
            let total: usize = times.iter().sum();
            let average = total / times.len();
            leaderboard.insert(area_id, average);
        }

        let mut areas: Vec<_> = leaderboard.into_iter().collect();
        if incrementing {
            areas.sort_by(|a, b| a.1.cmp(&b.1));
        } else {
            areas.sort_by(|a, b| b.1.cmp(&a.1));
        }
        let areas: Vec<_> = areas.into_iter().take(top_nth).collect();
        let mut leaderboard  = LeaderBoard { items: Vec::with_capacity(top_nth)};
        println!("Getting teaching area info: ");
        let areas_progress = ProgressBar::new(areas.len() as u64);
        for (area_id, avg_time) in areas {
            areas_progress.inc(1);
            let pros_area = ProsArea::get_pros_area(area_id, church_client).await?;
            let leaderboard_item = LeaderBoardItem::new(pros_area.missionaries, pros_area.area_name, avg_time);
            leaderboard.items.push(leaderboard_item);
        }
        areas_progress.finish();
        Ok(leaderboard)
    }
    pub fn pretty_print(&self) {
        let mut place = 0;
        for item in &self.items {
            place += 1;
            let area_name = &item.area_name;
            let missionaries: Vec<String> = item.missionaries
                .iter()
                .map(|missionary| {
                    missionary.pretty_print()
                })
                .collect();
            let missionaries = missionaries.join(" & ");
            println!("{place}: {area_name} ({missionaries}): {} sec", item.response_time)
        }
    }
}