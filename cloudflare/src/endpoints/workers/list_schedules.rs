use super::WorkersSchedule;

use crate::framework::endpoint::{EndpointSpec, Method};

/// List Schedules
/// <https://developers.cloudflare.com/api/resources/workers/subresources/scripts/subresources/schedules/methods/get/>
#[derive(Debug)]
pub struct ListSchedules<'a> {
    /// Account ID of owner of the script
    pub account_identifier: &'a str,
    /// The name of the script to list the schedules
    pub script_name: &'a str,
}

impl<'a> EndpointSpec<Vec<WorkersSchedule>> for ListSchedules<'a> {
    fn method(&self) -> Method {
        Method::GET
    }

    fn path(&self) -> String {
        format!(
            "accounts/{}/workers/scripts/{}/schedules",
            self.account_identifier, self.script_name
        )
    }
}
