//! Integrations with the OS and outside services. Each is a client of the
//! engine like any UI: it watches engine state and sends commands.

pub mod discord;
pub mod media_controls;
pub mod settings;
