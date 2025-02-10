use axum::{
    extract::{Query, State},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Deserialize;

use crate::{cloudflare_client, get_zone, libdns::LibDnsRecord, LinkupState};

pub fn router() -> Router<LinkupState> {
    Router::new().route(
        "/linkup/certificate-dns",
        get(get_certificate_dns_handler)
            .post(create_certificate_dns_handler)
            .put(update_certificate_dns_handler)
            .delete(delete_certificate_dns_handler),
    )
}

#[derive(Deserialize)]
struct GetCertificateDns {
    zone: String,
}

#[worker::send]
async fn get_certificate_dns_handler(
    State(state): State<LinkupState>,
    Query(query): Query<GetCertificateDns>,
) -> impl IntoResponse {
    let client = cloudflare_client(&state.cloudflare.api_token);

    let zone = get_zone(&client, &query.zone).await;

    let req = cloudflare::endpoints::dns::ListDnsRecords {
        zone_identifier: &zone.id,
        params: cloudflare::endpoints::dns::ListDnsRecordsParams::default(),
    };

    let records = client.request(&req).await.unwrap().result;
    let mut libdns_records: Vec<LibDnsRecord> = Vec::with_capacity(records.len());
    for record in records {
        libdns_records.push(record.into());
    }

    Json(libdns_records)
}

#[derive(Debug, Deserialize)]
struct CreateDnsRecords {
    zone: String,
    records: Vec<LibDnsRecord>,
}

#[worker::send]
async fn create_certificate_dns_handler(
    State(state): State<LinkupState>,
    Json(payload): Json<CreateDnsRecords>,
) -> impl IntoResponse {
    let client = cloudflare_client(&state.cloudflare.api_token);

    let zone = get_zone(&client, &payload.zone).await;

    let mut records: Vec<LibDnsRecord> = Vec::with_capacity(payload.records.len());

    for record in payload.records {
        let create_record = cloudflare::endpoints::dns::CreateDnsRecord {
            zone_identifier: &zone.id,
            params: (&record).into(),
        };

        let response = client.request(&create_record).await.unwrap().result;

        records.push(response.into());
    }

    Json(records)
}

#[derive(Debug, Deserialize)]
struct UpdateDnsRecords {
    zone: String,
    records: Vec<LibDnsRecord>,
}

#[worker::send]
async fn update_certificate_dns_handler(
    State(state): State<LinkupState>,
    Json(payload): Json<UpdateDnsRecords>,
) -> impl IntoResponse {
    let client = cloudflare_client(&state.cloudflare.api_token);

    let zone = get_zone(&client, &payload.zone).await;

    let mut updated_records: Vec<LibDnsRecord> = Vec::with_capacity(payload.records.len());
    for record in payload.records {
        if record.id.is_empty() {
            // TODO: Check if we need to implement this for our use case.
            unimplemented!("Needs to implement lookup DNS by name and type");
        }

        let req = cloudflare::endpoints::dns::UpdateDnsRecord {
            zone_identifier: &zone.id,
            identifier: &record.id,
            params: (&record).into(),
        };

        let res = client.request(&req).await.unwrap().result;
        updated_records.push(res.into());
    }

    Json(updated_records)
}

#[derive(Debug, Deserialize)]
struct DeleteDnsRecords {
    zone: String,
    records: Vec<LibDnsRecord>,
}

#[worker::send]
async fn delete_certificate_dns_handler(
    State(state): State<LinkupState>,
    Json(payload): Json<DeleteDnsRecords>,
) -> impl IntoResponse {
    let client = cloudflare_client(&state.cloudflare.api_token);

    let zone = get_zone(&client, &payload.zone).await;

    let mut deleted_records = Vec::with_capacity(payload.records.len());
    for record in payload.records {
        if record.id.is_empty() {
            // TODO: Check if we need to implement this for our use case.
            unimplemented!("Needs to implement lookup DNS by name and type");
        }

        let req = cloudflare::endpoints::dns::DeleteDnsRecord {
            zone_identifier: &zone.id,
            identifier: &record.id,
        };

        client.request(&req).await.unwrap();

        deleted_records.push(record);
    }

    Json(deleted_records)
}
