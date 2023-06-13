//! Middlewares for rewriting HTML using [`tower`] and [`lol_html`].
//!
//! The current interface is both unstable AND moderately unsafe.
//! Please do not use this crate directly until it has a
//! non-prerelease version!

#![feature(impl_trait_in_assoc_type)]
#![forbid(unused_unsafe)]
#![warn(clippy::all, missing_docs, nonstandard_style, future_incompatible)]
#![allow(clippy::type_complexity)]

pub mod resolve;
pub mod rewrite;
mod util;

pub use lol_html;
pub use resolve::{ResolveLayer, ResolveService};
pub use rewrite::{HtmlRewriteLayer, HtmlRewriteService};
