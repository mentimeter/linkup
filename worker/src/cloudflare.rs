// TODO: This is also an implementation of a client to Cloudflare. This could probably be merged with the api.rs in linkup-cli deploy command.
//   It probably need to be a separate crate, so that we can import on both the cli and the worker.

pub struct Client {}

pub trait DnsApi {
    async fn create_record();
    async fn update_record();
    async fn delete_record();
    async fn list_records();
}

impl Client {
    fn new() -> Self {
        todo!()
    }
}

impl DnsApi for Client {
    async fn create_record() {
        todo!()
    }

    async fn update_record() {
        todo!()
    }

    async fn delete_record() {
        todo!()
    }

    async fn list_records() {
        todo!()
    }
}
