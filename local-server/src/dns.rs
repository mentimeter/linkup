use std::{collections::BTreeSet, net::Ipv4Addr, str::FromStr, sync::Arc};

use hickory_server::{
    net::runtime::{Time, TokioRuntimeProvider},
    proto::rr::{LowerName, Name, RData, Record},
    server::{Request, RequestHandler, ResponseHandler, ResponseInfo},
    store::in_memory::InMemoryZoneHandler,
    zone_handler::{AxfrPolicy, Catalog, ZoneHandler, ZoneType},
};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct DnsCatalog {
    catalog: Arc<RwLock<Catalog>>,
    domains: Arc<RwLock<BTreeSet<String>>>,
}

impl DnsCatalog {
    pub fn new() -> Self {
        Self {
            catalog: Arc::new(RwLock::new(Catalog::new())),
            domains: Arc::new(RwLock::new(BTreeSet::new())),
        }
    }

    pub async fn list_domains(&self) -> Vec<String> {
        self.domains.read().await.iter().cloned().collect()
    }

    pub(crate) async fn upsert_zone(&self, name: LowerName, handlers: Vec<Arc<dyn ZoneHandler>>) {
        self.catalog.write().await.upsert(name, handlers);
    }

    pub(crate) async fn remove_zone(&self, name: &LowerName) {
        self.catalog.write().await.remove(name);
    }
}

impl Default for DnsCatalog {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl RequestHandler for DnsCatalog {
    async fn handle_request<R: ResponseHandler, T: Time>(
        &self,
        request: &Request,
        response_handle: R,
    ) -> ResponseInfo {
        let catalog = self.catalog.read().await;

        catalog
            .handle_request::<R, T>(request, response_handle)
            .await
    }
}

pub async fn register_dns_record(dns_catalog: &DnsCatalog, domain: &str) {
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

    dns_catalog
        .upsert_zone(record_name.into(), vec![Arc::new(authority)])
        .await;

    dns_catalog.domains.write().await.insert(domain.to_string());
}

pub async fn deregister_dns_record(dns_catalog: &DnsCatalog, domain: &str) {
    let record_name = Name::from_str(&format!("{}.", domain))
        .expect("dns record from domain should always succeed");

    dns_catalog.remove_zone(&record_name.into()).await;

    dns_catalog.domains.write().await.remove(domain);
}
