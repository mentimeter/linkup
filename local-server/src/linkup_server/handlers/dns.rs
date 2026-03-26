use std::{net::Ipv4Addr, str::FromStr, sync::Arc};

use axum::{response::IntoResponse, Extension, Json};
use hickory_server::{
    authority::ZoneType,
    proto::rr::{RData, Record},
    resolver::Name,
    store::in_memory::InMemoryAuthority,
};
use http::StatusCode;
use serde::Deserialize;

use crate::dns_server::DnsCatalog;

#[derive(Deserialize)]
pub struct CreateDnsRecord {
    pub domain: String,
}

pub async fn handle_create(
    Extension(dns_catalog): Extension<DnsCatalog>,
    Json(payload): Json<CreateDnsRecord>,
) -> impl IntoResponse {
    let mut catalog = dns_catalog.write().await;

    let record_name = Name::from_str(&format!("{}.", payload.domain)).unwrap();

    let authority = InMemoryAuthority::empty(record_name.clone(), ZoneType::Primary, false);

    let record = Record::from_rdata(
        record_name.clone(),
        3600,
        RData::A(Ipv4Addr::new(127, 0, 0, 1).into()),
    );

    authority.upsert(record, 0).await;

    catalog.upsert(record_name.clone().into(), vec![Arc::new(authority)]);

    StatusCode::CREATED.into_response()
}
