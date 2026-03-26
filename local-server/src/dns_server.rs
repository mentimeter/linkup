use hickory_server::{
    authority::Catalog,
    proto::{rr::Name, xfer::Protocol},
    resolver::{
        config::{NameServerConfig, NameServerConfigGroup, ResolverOpts},
        name_server::TokioConnectionProvider,
    },
    server::{RequestHandler, ResponseHandler, ResponseInfo},
    store::forwarder::{ForwardAuthority, ForwardConfig},
    ServerFuture,
};
use std::sync::Arc;
use std::{net::SocketAddr, ops::Deref};
use tokio::{net::UdpSocket, sync::RwLock};

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
    async fn handle_request<R: ResponseHandler>(
        &self,
        request: &hickory_server::server::Request,
        response_handle: R,
    ) -> ResponseInfo {
        let catalog = self.read().await;

        catalog.handle_request(request, response_handle).await
    }
}

pub async fn serve(dns_catalog: DnsCatalog) {
    let cf_name_server = NameServerConfig::new("1.1.1.1:53".parse().unwrap(), Protocol::Udp);
    let forward_config = ForwardConfig {
        name_servers: NameServerConfigGroup::from(vec![cf_name_server]),
        options: Some(ResolverOpts::default()),
    };

    let forwarder =
        ForwardAuthority::builder_with_config(forward_config, TokioConnectionProvider::default())
            .with_origin(Name::root())
            .build()
            .unwrap();

    {
        let mut catalog = dns_catalog.write().await;
        catalog.upsert(Name::root().into(), vec![Arc::new(forwarder)]);
    }

    let addr = SocketAddr::from(([0, 0, 0, 0], 8053));
    let sock = UdpSocket::bind(&addr).await.unwrap();

    let mut server = ServerFuture::new(dns_catalog);
    server.register_socket(sock);

    println!("DNS server listening on {addr}");
    server.block_until_done().await.unwrap();
}
