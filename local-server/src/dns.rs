use std::{net::Ipv4Addr, ops::Deref, str::FromStr, sync::Arc};

use hickory_server::{
    net::runtime::{Time, TokioRuntimeProvider},
    proto::rr::{Name, RData, Record},
    server::{Request, RequestHandler, ResponseHandler, ResponseInfo},
    store::in_memory::InMemoryZoneHandler,
    zone_handler::{AxfrPolicy, Catalog, ZoneType},
};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct DnsCatalog(Arc<RwLock<Catalog>>);

impl DnsCatalog {
    pub fn new() -> Self {
        Self(Arc::new(RwLock::new(Catalog::new())))
    }
}

impl Default for DnsCatalog {
    fn default() -> Self {
        Self::new()
    }
}

impl Deref for DnsCatalog {
    type Target = Arc<RwLock<Catalog>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[async_trait::async_trait]
impl RequestHandler for DnsCatalog {
    async fn handle_request<R: ResponseHandler, T: Time>(
        &self,
        request: &Request,
        response_handle: R,
    ) -> ResponseInfo {
        let catalog = self.read().await;

        catalog
            .handle_request::<R, T>(request, response_handle)
            .await
    }
}

pub async fn register_dns_record(dns_catalog: &DnsCatalog, domain: &str) {
    let mut catalog = dns_catalog.write().await;

    let record_name = Name::from_str(&format!("{}.", domain))
        .expect("dns record from domain should always succeed");

    let authority: InMemoryZoneHandler<TokioRuntimeProvider> =
        InMemoryZoneHandler::empty(record_name.clone(), ZoneType::Primary, AxfrPolicy::Deny);

    let record = Record::from_rdata(
        record_name.clone(),
        3600,
        RData::A(Ipv4Addr::new(127, 0, 0, 1).into()),
    );

    authority.upsert(record, 0).await;

    catalog.upsert(record_name.clone().into(), vec![Arc::new(authority)]);
}
