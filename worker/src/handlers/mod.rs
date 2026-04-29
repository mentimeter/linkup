pub mod proxy;
pub mod v1;
pub mod v2;

pub async fn always_ok() -> &'static str {
    "OK"
}
