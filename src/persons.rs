// Jackson Coxson

use crate::{cache::PersonsCacheMap, church::ChurchClient, holly::scheduled_times::RefetchPolicy};
use chrono::naive::serde::ts_milliseconds;
use chrono::NaiveDateTime;
use log::warn;
use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

use crate::cache;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Persons {
    persons: Vec<Person>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Person {
    #[serde(rename = "personGuid")]
    pub guid: String,

    #[serde(rename = "firstName")]
    pub first_name: String,

    #[serde(rename = "referralStatusId")]
    pub referral_status: ReferralStatus,

    #[serde(rename = "personStatusId")]
    pub person_status: PersonStatus,

    #[serde(rename = "missionId")]
    pub mission_id: usize,

    #[serde(rename = "zoneId")]
    pub zone_id: Option<usize>,

    #[serde(rename = "zoneName")]
    pub zone_name: Option<String>,

    #[serde(rename = "districtId")]
    pub district_id: Option<usize>,

    #[serde(rename = "areaName")]
    pub area_name: Option<String>,

    #[serde(rename = "areaId")]
    pub area_id: Option<usize>,

    #[serde(rename = "referralAssignedDate")]
    #[serde(with = "ts_milliseconds")]
    pub assigned_date: NaiveDateTime,

    // #[serde(with = "ts_milliseconds")]
    #[serde(rename = "baptismGoalDate")]
    pub baptism_goal_date: Option<NaiveDateTime>,

    // #[serde(with = "ts_milliseconds")]
    #[serde(rename = "baptismDate")]
    pub baptism_date: Option<NaiveDateTime>,

    // #[serde(with = "ts_milliseconds")]
    #[serde(rename = "lastSacramentDate")]
    pub last_sacrament_attendance: Option<NaiveDateTime>,

    pub response_time: Option<usize>,

    pub has_attended_sacrament: Option<bool>,

    pub has_attended_since_last_referral: Option<bool>,

    pub sacrament_attendence_count: Option<usize>,

    pub has_been_taught: Option<bool>,

    pub has_been_taught_in_person: Option<bool>,

    pub has_been_taught_since_last_referral: Option<bool>,

    pub referral_count: Option<usize>,

    pub last_timeline_assessment: Option<NaiveDateTime>,
}

impl Person {
    pub fn parse_lossy(mut object: serde_json::Value) -> Vec<Self> {
        if let serde_json::Value::Array(persons) = object["persons"].take() {
            let mut res: Vec<Self> = Vec::with_capacity(persons.len());
            for person in persons {
                if let Ok(p) = serde_json::from_value(person.clone()) {
                    res.push(p);
                } else {
                    warn!("Unable to parse person: {person:?}");
                }
            }
            res
        } else {
            Vec::new()
        }
    }

    pub async fn apply_cache(
        &mut self,
        church_client: &mut ChurchClient,
        cache_map: &mut PersonsCacheMap,
        refetch_policy: RefetchPolicy,
    ) -> &mut Self {
        if let Ok(Some(cache)) = cache::PersonCache::get_cached_data_for_person(
            church_client,
            &self.clone(),
            cache_map,
            refetch_policy,
        )
        .await
        {
            self.response_time = cache.response_time;
            self.has_attended_sacrament = Some(cache.has_attended_sacrament);
            self.has_attended_since_last_referral = Some(cache.has_attended_since_last_referral);
            self.sacrament_attendence_count = Some(cache.sacrament_attendance_count);
            self.has_been_taught = Some(cache.has_been_taught);
            self.has_been_taught_in_person = Some(cache.has_been_taught_in_person);
            self.has_been_taught_since_last_referral =
                Some(cache.has_been_taught_since_last_referral);
            self.referral_count = cache.referral_count;
            self.last_timeline_assessment = Some(cache.last_timeline_assessment);
        }
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TimelineEvent {
    #[serde(rename = "timelineItemType")]
    pub item_type: TimelineItemType,

    #[serde(rename = "itemDate")]
    #[serde(with = "ts_milliseconds")]
    pub item_date: NaiveDateTime,

    #[serde(rename = "contactTypeCode")]
    pub contact_type: Option<TimelineContactType>,

    #[serde(rename = "eventStatus")]
    pub status: Option<bool>,
}

impl TimelineEvent {
    pub fn parse_lossy(object: serde_json::Value) -> Vec<Self> {
        if let serde_json::Value::Array(persons) = object {
            let mut res: Vec<Self> = Vec::with_capacity(persons.len());
            for person in persons {
                if let Ok(p) = serde_json::from_value(person.clone()) {
                    res.push(p);
                } else {
                    warn!("Unable to parse timeline event: {person:?}");
                }
            }
            res
        } else {
            Vec::new()
        }
    }
}

#[derive(Serialize_repr, Deserialize_repr, PartialEq, Clone, Debug)]
#[repr(u8)]
pub enum ReferralStatus {
    NotAttempted = 10,
    NotSuccessful = 20,
    Successful = 30,
}

#[derive(Serialize_repr, Deserialize_repr, PartialEq, Clone, Debug, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum PersonStatus {
    Yellow = 1,
    Green = 2,
    BetterGreen = 3,
    ProgressingGreen = 4,
    NewMember = 6,
    NotInterested = 20,
    NotInterestedDeclared = 21,
    NotProgressing = 22,
    UnableToContact = 23,
    Prank = 25, // unsure
    NotRecentlyContacted = 26,
    TooBusy = 27,
    OutsideAreaStrength = 28,
    Member = 40,
    Moved = 201,
}

#[derive(PartialEq, Clone, Debug, Serialize, Deserialize)]
pub enum TimelineContactType {
    #[serde(rename = "PERSON")]
    InPerson,
    #[serde(rename = "PHONE")]
    Phone,
    #[serde(rename = "TEXT")]
    Text,
    #[serde(rename = "WHATSAPP")]
    WhatsApp,
    #[serde(rename = "EMAIL")]
    Email,
    #[serde(rename = "VIDEO_CALL")]
    VideoCall,
}

#[derive(PartialEq, Clone, Debug, Serialize, Deserialize)]
pub enum TimelineItemType {
    #[serde(rename = "STOPPED_TEACHING")]
    StoppedTeaching,
    #[serde(rename = "CONTACT")]
    Contact,
    #[serde(rename = "TEACHING")]
    Teaching,
    #[serde(rename = "NEW_REFERRAL")]
    NewReferral,
    #[serde(rename = "PERSON_CREATE")]
    PersonCreate,
    #[serde(rename = "PERSON_OFFER_ITEM")]
    PersonOfferItem,
    #[serde(rename = "SACRAMENT")]
    Sacrament,
    #[serde(rename = "TEACHING_RESET")]
    TeachingReset,
    #[serde(rename = "PERSON_PLN_NOTE")]
    Note,
    #[serde(rename = "PERSON_TASK")]
    Task,
    #[serde(rename = "EMAIL_SUBSCRIPTION")]
    EmailSubscription,
}

#[cfg(test)]
mod tests {
    #[test]
    fn t1() {
        let list = std::fs::read_to_string("list.json").unwrap();
        let list = super::Person::parse_lossy(serde_json::from_str(&list).unwrap());
        println!("{list:?}");
    }
}
