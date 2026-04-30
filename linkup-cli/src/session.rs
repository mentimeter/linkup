use colored::Colorize;
use linkup::{Domain, SessionKind};
use serde::{Deserialize, Serialize};

use crate::state::State;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionRow {
    pub name: String,
    pub kind: SessionKind,
    pub domains: Vec<String>,
}

impl SessionRow {
    pub fn from_state(state: &State, kind: SessionKind) -> Self {
        SessionRow {
            name: state.linkup.session_name.clone(),
            kind,
            domains: format_state_domains(&state.linkup.session_name, &state.domains),
        }
    }
}

pub fn print_sessions_table(sessions: &[SessionRow], current: Option<&str>) {
    if sessions.is_empty() {
        return;
    }

    let has_marker = current.is_some();
    let marker_len = if has_marker { 2 } else { 0 };

    let name_width = sessions
        .iter()
        .map(|s| s.name.len() + marker_len)
        .chain(std::iter::once("SESSION NAME".len() + marker_len))
        .max()
        .unwrap_or(0);

    let kind_width = sessions
        .iter()
        .map(|s| s.kind.as_str().len())
        .chain(std::iter::once("TYPE".len()))
        .max()
        .unwrap_or(0);

    let domain_width = sessions
        .iter()
        .flat_map(|s| s.domains.iter().map(|d| d.len()))
        .chain(std::iter::once("DOMAINS".len()))
        .max()
        .unwrap_or(0);

    let border = |left: &str, mid: &str, right: &str| {
        format!(
            "{}{}{}{}{}{}{}",
            left,
            "─".repeat(name_width + 2),
            mid,
            "─".repeat(kind_width + 2),
            mid,
            "─".repeat(domain_width + 2),
            right,
        )
    };

    let top = border("┌", "┬", "┐");
    let mid = border("├", "┼", "┤");
    let bottom = border("└", "┴", "┘");

    println!("{}", top);

    let header_name_str = if has_marker {
        format!("{}SESSION NAME", " ".repeat(marker_len))
    } else {
        "SESSION NAME".to_string()
    };

    println!(
        "│ {} │ {} │ {} │",
        format!("{:<w$}", header_name_str, w = name_width).bold(),
        format!("{:<w$}", "TYPE", w = kind_width).bold(),
        format!("{:<w$}", "DOMAINS", w = domain_width).bold(),
    );

    println!("{}", mid);

    for (i, entry) in sessions.iter().enumerate() {
        let is_current = current.map(|c| c == entry.name).unwrap_or(false);

        let name_with_marker = if has_marker {
            if is_current {
                format!("> {}", entry.name)
            } else {
                format!("  {}", entry.name)
            }
        } else {
            entry.name.clone()
        };

        let name_padded = format!("{:<w$}", name_with_marker, w = name_width);
        let name_display = if is_current {
            name_padded.bold().to_string()
        } else {
            name_padded
        };

        for (i, domain) in entry.domains.iter().enumerate() {
            if i == 0 {
                println!(
                    "│ {} │ {:<kind_width$} │ {:<domain_width$} │",
                    name_display, entry.kind, domain,
                );
            } else {
                println!(
                    "│ {:<name_width$} │ {:<kind_width$} │ {:<domain_width$} │",
                    "", "", domain,
                );
            }
        }

        if i < sessions.len() - 1 {
            println!("{}", mid);
        }
    }

    println!("{}", bottom);
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
