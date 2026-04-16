pub mod proxy;
pub mod v1;

pub async fn always_ok() -> &'static str {
    "OK"
}
