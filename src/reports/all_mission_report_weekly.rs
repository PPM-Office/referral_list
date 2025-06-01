use std::collections::HashMap;

pub struct AllMissionReportWeekly {
    pub zone_avg_response_time: HashMap<String, (usize, usize)>,
    pub percent_referrals_taught: f32,
    pub numb_of_referrals_at_sacrament: usize
}

impl AllMissionReportWeekly {
}