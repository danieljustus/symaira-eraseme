//! Strict schema-v1 broker registry models, loading, filtering, and sync.

mod filter;
mod loader;
mod model;
mod sync;
mod validate;

pub use filter::{BrokerFilter, filter_brokers, filter_brokers_owned};
pub use loader::{load, load_embedded, load_from_dir};
pub use model::{
    Broker, CaptchaProvider, CaptchaType, Category, Channel, ChannelType, FormSpec, FormStep,
    Jurisdiction, Law, Priority, RequiredField, SolveCaptcha, Status, Template, Verification,
};
pub use sync::{SyncResponse, SyncTransport, sync, sync_with_transport};
pub use validate::RegistryError;
