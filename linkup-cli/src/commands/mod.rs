pub mod completion;
pub mod deploy;
pub mod health;
pub mod local;
#[cfg(target_os = "macos")]
pub mod local_dns;
pub mod preview;
pub mod remote;
pub mod reset;
pub mod server;
pub mod start;
pub mod status;
pub mod stop;
pub mod uninstall;
pub mod update;

pub use {completion::completion, completion::Args as CompletionArgs};
pub use {deploy::deploy, deploy::DeployArgs};
pub use {deploy::destroy, deploy::DestroyArgs};
pub use {health::health, health::Args as HealthArgs};
pub use {local::local, local::Args as LocalArgs};
#[cfg(target_os = "macos")]
pub use {local_dns::local_dns, local_dns::Args as LocalDnsArgs};
pub use {preview::preview, preview::Args as PreviewArgs};
pub use {remote::remote, remote::Args as RemoteArgs};
pub use {reset::reset, reset::Args as ResetArgs};
pub use {server::server, server::Args as ServerArgs};
pub use {start::start, start::Args as StartArgs};
pub use {status::status, status::Args as StatusArgs};
pub use {stop::stop, stop::Args as StopArgs};
pub use {uninstall::uninstall, uninstall::Args as UninstallArgs};
pub use {update::update, update::Args as UpdateArgs};
