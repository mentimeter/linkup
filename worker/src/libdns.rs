// This module behaves as the glue between [libdns](https://github.com/libdns/libdns) (used by Caddy) and Cloudflare
// TODO: Maybe upstream this as a provider under a repo like https://github.com/lus/libdns-rs

use cloudflare::endpoints::dns::{
    CreateDnsRecordParams as CfCreateDnsRecordParams, DnsContent as CfDnsContent,
    DnsRecord as CfDnsRecord, UpdateDnsRecordParams as CfUpdateDnsRecordParams,
};
use serde::{Deserialize, Serialize};

/// This represents the record that is used in Caddy for working with libdns.
///
/// Reference: https://github.com/libdns/libdns/blob/8b75c024f21e77c1ee32273ad24c579d1379b2b0/libdns.go#L114-L127
#[derive(Debug, Serialize, Deserialize)]
pub struct LibDnsRecord {
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "Type")]
    pub record_type: String,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Value")]
    pub value: String,
    #[serde(rename = "TTL")]
    pub ttl: u32,
    #[serde(rename = "Priority")]
    pub priority: u16,
    #[serde(rename = "Weight")]
    pub weight: u32,
}

impl From<CfDnsRecord> for LibDnsRecord {
    fn from(value: CfDnsRecord) -> Self {
        let (ty, content_value, priority) = match value.content {
            CfDnsContent::A { content } => ("A", content.to_string(), None),
            CfDnsContent::AAAA { content } => ("AAAA", content.to_string(), None),
            CfDnsContent::CNAME { content } => ("CNAME", content, None),
            CfDnsContent::NS { content } => ("NS", content, None),
            CfDnsContent::MX { content, priority } => ("MX", content, Some(priority)),
            CfDnsContent::TXT { content } => ("TXT", content, None),
            CfDnsContent::SRV { content } => ("SRV", content, None),
        };

        Self {
            id: value.id,
            record_type: ty.to_string(),
            name: value.name,
            value: content_value,
            ttl: value.ttl,
            priority: priority.unwrap_or(0),
            weight: 0,
        }
    }
}

impl<'a> From<&'a LibDnsRecord> for CfCreateDnsRecordParams<'a> {
    fn from(val: &'a LibDnsRecord) -> Self {
        CfCreateDnsRecordParams {
            ttl: Some(val.ttl),
            priority: Some(val.priority),
            proxied: Some(false),
            name: &val.name,
            content: match val.record_type.as_str() {
                "A" => cloudflare::endpoints::dns::DnsContent::A {
                    content: val.value.parse().unwrap(),
                },
                "AAAA" => cloudflare::endpoints::dns::DnsContent::AAAA {
                    content: val.value.parse().unwrap(),
                },
                "CNAME" => cloudflare::endpoints::dns::DnsContent::CNAME {
                    content: val.value.clone(),
                },
                "NS" => cloudflare::endpoints::dns::DnsContent::NS {
                    content: val.value.clone(),
                },
                "MX" => cloudflare::endpoints::dns::DnsContent::MX {
                    content: val.value.clone(),
                    priority: val.priority,
                },
                "TXT" => cloudflare::endpoints::dns::DnsContent::TXT {
                    content: val.value.clone(),
                },
                "SRV" => cloudflare::endpoints::dns::DnsContent::SRV {
                    content: val.value.clone(),
                },
                _ => unreachable!(),
            },
        }
    }
}

impl<'a> From<&'a LibDnsRecord> for CfUpdateDnsRecordParams<'a> {
    fn from(val: &'a LibDnsRecord) -> Self {
        CfUpdateDnsRecordParams {
            ttl: Some(val.ttl),
            proxied: Some(false),
            name: &val.name,
            content: match val.record_type.as_str() {
                "A" => cloudflare::endpoints::dns::DnsContent::A {
                    content: val.value.parse().unwrap(),
                },
                "AAAA" => cloudflare::endpoints::dns::DnsContent::AAAA {
                    content: val.value.parse().unwrap(),
                },
                "CNAME" => cloudflare::endpoints::dns::DnsContent::CNAME {
                    content: val.value.clone(),
                },
                "NS" => cloudflare::endpoints::dns::DnsContent::NS {
                    content: val.value.clone(),
                },
                "MX" => cloudflare::endpoints::dns::DnsContent::MX {
                    content: val.value.clone(),
                    priority: val.priority,
                },
                "TXT" => cloudflare::endpoints::dns::DnsContent::TXT {
                    content: val.value.clone(),
                },
                "SRV" => cloudflare::endpoints::dns::DnsContent::SRV {
                    content: val.value.clone(),
                },
                _ => unreachable!(),
            },
        }
    }
}
