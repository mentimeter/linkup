use std::{collections::BTreeSet, net::Ipv4Addr, str::FromStr, sync::Arc};

use hickory_server::{
    net::runtime::{Time, TokioRuntimeProvider},
    proto::rr::{Name, RData, Record},
    resolver::config::{NameServerConfig, ResolverOpts},
    server::{Request, RequestHandler, ResponseHandler, ResponseInfo},
    store::{
        forwarder::{ForwardConfig, ForwardZoneHandler},
        in_memory::InMemoryZoneHandler,
    },
    zone_handler::{AxfrPolicy, Catalog, ZoneType},
};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct DnsCatalog {
    catalog: Arc<RwLock<Catalog>>,
    domains: Arc<RwLock<BTreeSet<String>>>,
}

impl DnsCatalog {
    pub fn new() -> Self {
        let mut catalog = Catalog::new();

        let forward_config = ForwardConfig {
            name_servers: vec![NameServerConfig::udp(
                "1.1.1.1"
                    .parse()
                    .expect("1.1.1.1 should be a valid IP address"),
            )],
            options: Some(ResolverOpts::default()),
        };

        let forwarder = ForwardZoneHandler::builder_with_config(
            forward_config,
            TokioRuntimeProvider::default(),
        )
        .with_origin(Name::root())
        .build()
        .expect("ZoneHandler should be buildable with the current settings");

        catalog.upsert(Name::root().into(), vec![Arc::new(forwarder)]);

        Self {
            catalog: Arc::new(RwLock::new(catalog)),
            domains: Arc::new(RwLock::new(BTreeSet::new())),
        }
    }

    pub async fn list_domains(&self) -> Vec<String> {
        self.domains.read().await.iter().cloned().collect()
    }

    pub async fn register_record(&self, domain: &str) {
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

        self.catalog
            .write()
            .await
            .upsert(record_name.into(), vec![Arc::new(authority)]);
        self.domains.write().await.insert(domain.to_string());
    }

    pub async fn deregister_record(&self, domain: &str) {
        let record_name = Name::from_str(&format!("{}.", domain))
            .expect("dns record from domain should always succeed");

        self.catalog.write().await.remove(&record_name.into());
        self.domains.write().await.remove(domain);
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
