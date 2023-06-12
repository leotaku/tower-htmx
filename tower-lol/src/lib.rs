//! Middlewares for rewriting HTML using [`tower`] and [`lol_html`].
//!
//! The current interface is both unstable AND moderately unsafe.
//! Please do not use this crate directly until it has a
//! non-prerelease version!

#![feature(type_alias_impl_trait)]
#![forbid(unused_unsafe)]
#![warn(clippy::all, missing_docs, nonstandard_style, future_incompatible)]
#![allow(clippy::type_complexity)]

pub mod resolve;
pub mod rewriter;
mod util;

pub use lol_html;
pub use resolve::{ResolveLayer, ResolveService};
pub use rewriter::{HtmlRewriterLayer, HtmlRewriterService};
