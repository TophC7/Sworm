pub mod auth;
pub mod config;
mod dispatch;
mod events;
mod file_stream;
mod lsp_stream;
mod pty_stream;
mod server;
mod stream;
#[cfg(test)]
pub(crate) mod test_support;
mod web;
mod workbenches;

pub use server::{serve, ServeOptions, ServerHandle};
