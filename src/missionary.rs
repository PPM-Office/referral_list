use std::fmt;
use serde::{Deserialize, Serialize};


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Missionary {
    #[serde(rename = "firstName")]
    pub first_name: String,
    #[serde(rename = "lastName")]
    pub last_name: String,
    #[serde(rename = "missionaryType")]
    missionary_type: MissionaryType,
    #[serde(rename = "prosAreaId")]
    pros_area_id: usize
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProsArea {
    pub id: usize,
    #[serde(rename = "name")]
    pub area_name: String,
    pub missionaries: Vec<Missionary>
}

#[derive(Clone, Debug, Serialize, Deserialize)]
enum MissionaryType {
    #[serde(rename = "ELDER")]
    Elder,
    #[serde(rename = "SISTER")]
    Sister
}

impl fmt::Display for MissionaryType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let title = match self {
            MissionaryType::Elder => "Elder",
            MissionaryType::Sister => "Sister",
        };
        write!(f, "{}", title)
    }
}

impl ProsArea {
    pub async fn get_pros_area(area_id: usize, church_client: &mut crate::ChurchClient) -> anyhow::Result<Self> {
        church_client.refresh_auth().await?;
        let result = church_client
            .http_client
            .get(format!("https://referralmanager.churchofjesuschrist.org/services/mission/prosArea/{}", area_id as u64))
            .header("Accept", "application/json")
            .send()
            .await?
            .json::<ProsArea>()
            .await;
    
        Ok(result?)
    }
}

impl Missionary {
    pub fn pretty_print(&self) -> String {
        format!("{} {}", self.missionary_type, self.last_name)
    }
}