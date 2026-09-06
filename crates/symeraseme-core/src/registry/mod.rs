//! Strict schema-v1 broker registry models and validation.

mod model;
mod validate;

pub use model::{
    Broker, CaptchaProvider, CaptchaType, Category, Channel, ChannelType, FormSpec, FormStep,
    Jurisdiction, Law, Priority, RequiredField, SolveCaptcha, Status, Template, Verification,
};
pub use validate::{RegistryError, load, load_from_dir};
