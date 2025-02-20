use super::WorkersSchedule;

use crate::framework::endpoint::{EndpointSpec, Method};

use serde::Serialize;

/// Upsert Schedules
/// <https://developers.cloudflare.com/api/resources/workers/subresources/scripts/subresources/schedules/methods/update/>
#[derive(Debug)]
pub struct UpsertSchedules<'a> {
    /// Account ID of owner of the script
    pub account_identifier: &'a str,
    /// The name of the script to upsert the schedules
    pub script_name: &'a str,
    /// Params for upserting the schedules
    pub params: UpsertSchedulesParams,
}

impl<'a> EndpointSpec<Vec<WorkersSchedule>> for UpsertSchedules<'a> {
    fn method(&self) -> Method {
        Method::PUT
    }

    fn path(&self) -> String {
        format!(
            "accounts/{}/workers/scripts/{}/schedules",
            self.account_identifier, self.script_name
        )
    }

    #[inline]
    fn body(&self) -> Option<String> {
        Some(serde_json::to_string(&self.params).unwrap())
    }
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct UpsertSchedulesParams {
    pub schedules: Vec<WorkersSchedule>,
}
