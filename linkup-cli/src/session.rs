use linkup::Domain;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionStatus {
    pub name: String,
    pub domains: Vec<String>,
}

impl SessionStatus {
    pub fn print(&self) {
        println!("Session Name: {}", self.name);
        println!("Domains:");
        for domain in &self.domains {
            println!("    {}", domain);
        }
    }
}

pub fn format_state_domains(session_name: &str, domains: &[Domain]) -> Vec<String> {
    let filtered: Vec<String> = domains
        .iter()
        .filter(|domain| {
            !domains.iter().any(|other| {
                other.domain != domain.domain && domain.domain.ends_with(&other.domain)
            })
        })
        .map(|domain| domain.domain.clone())
        .collect();

    filtered
        .iter()
        .map(|domain| format!("https://{}.{}", session_name, domain))
        .collect()
}
