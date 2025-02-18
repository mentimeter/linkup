use axum::{
    extract::{Query, State},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::Deserialize;

use crate::{cloudflare_client, libdns::LibDnsRecord, LinkupState};

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

    format_records_names(&mut libdns_records, &zone.name);

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

    format_records_names(&mut records, &zone.name);

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

        let req = cloudflare::endpoints::dns::PatchDnsRecord {
            zone_identifier: &zone.id,
            identifier: &record.id,
            params: (&record).into(),
        };

        let res = client.request(&req).await.unwrap().result;
        updated_records.push(res.into());
    }

    format_records_names(&mut updated_records, &zone.name);

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

    format_records_names(&mut deleted_records, &zone.name);

    Json(deleted_records)
}

async fn get_zone(
    client: &cloudflare::framework::async_api::Client,
    zone: &str,
) -> cloudflare::endpoints::zone::Zone {
    let req = cloudflare::endpoints::zone::ListZones {
        params: cloudflare::endpoints::zone::ListZonesParams {
            name: Some(zone.to_string()),
            ..Default::default()
        },
    };

    let mut res = client.request(&req).await.unwrap().result;
    if res.is_empty() {
        panic!("Zone not found");
    }

    if res.len() > 1 {
        panic!("Found more than one zone for name");
    }

    res.pop().unwrap()
}

fn format_records_names(records: &mut [LibDnsRecord], zone: &str) {
    for record in records.iter_mut() {
        record.name = name_relative_to_zone(&record.name, zone);
    }
}

fn name_relative_to_zone(fqdm: &str, zone: &str) -> String {
    let trimmed_fqdm = fqdm.trim_end_matches('.');
    let trimmed_zone = zone.trim_end_matches('.');

    let fqdm_relative_to_zone = trimmed_fqdm.replace(trimmed_zone, "");

    fqdm_relative_to_zone.trim_end_matches('.').to_string()
}

#[cfg(test)]
mod test {
    use crate::{
        libdns::LibDnsRecord,
        routes::certificate_dns::{format_records_names, name_relative_to_zone},
    };

    #[test]
    fn test_name_relative_to_zone() {
        let fqdm = "api.mentimeter.com.";
        let zone = "mentimeter.com.";

        assert_eq!("api", name_relative_to_zone(fqdm, zone));
    }

    #[test]
    fn test_name_relative_to_zone_subdomain() {
        let fqdm = "v2.api.mentimeter.com.";
        let zone = "mentimeter.com.";

        assert_eq!("v2.api", name_relative_to_zone(fqdm, zone));
    }

    #[test]
    fn test_name_relative_to_zone_not_matching_zone() {
        let fqdm = "api.mentimeter.com.";
        let zone = "menti.meter.";

        assert_eq!("api.mentimeter.com", name_relative_to_zone(fqdm, zone));
    }

    #[test]
    fn test_format_records_names() {
        let mut records = vec![LibDnsRecord {
            name: "api.mentimeter.com".to_string(),
            ..Default::default()
        }];
        let zone = "mentimeter.com";

        format_records_names(&mut records, zone);

        assert_eq!(records[0].name, "api")
    }
}
