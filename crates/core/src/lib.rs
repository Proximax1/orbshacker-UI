pub mod baked;
pub mod discord_db;
pub mod error;
pub mod fake;
pub mod http;
pub mod paths;
pub mod session;
pub mod settings;
pub mod steam;
pub mod timer;
pub mod window;

pub use error::{Error, Result};

pub const DEVELOPER: &str = "Strykey / Daniel Pires / Pannenkoekisus";
