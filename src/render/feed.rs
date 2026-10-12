use maud::{html, PreEscaped};

use super::{post_path, tag_path};
use crate::{config::Config, domain::PostSummary, time::LocalTime};

const XML_DECLARATION: PreEscaped<&str> = PreEscaped("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");

/// RSS 2.0 with absolute links; expects the newest post first.
pub fn rss(config: &Config, posts: &[PostSummary]) -> String {
    let site = &config.site;
    let base = &config.base_url;
    let rfc2822 = |timestamp: &str| {
        LocalTime::from_sqlite(timestamp, config.utc_offset_minutes).map(|time| time.rfc2822())
    };
    let last_build = posts.first().and_then(|post| rfc2822(post.date()));
    html! {
        (XML_DECLARATION)
        rss version="2.0" {
            channel {
                title { (site.title) }
                link { (base) "/" }
                description { (site.description) }
                language { (site.lang) }
                @if let Some(date) = &last_build {
                    lastBuildDate { (date) }
                }
                @for post in posts {
                    item {
                        title { (post.title) }
                        link { (base) (post_path(&post.slug)) }
                        guid isPermaLink="true" { (base) (post_path(&post.slug)) }
                        @if let Some(date) = rfc2822(post.date()) {
                            pubDate { (date) }
                        }
                        @if !post.excerpt.is_empty() {
                            description { (post.excerpt) }
                        }
                        @for tag in &post.tags {
                            category domain={ (base) (tag_path(tag)) } { (tag) }
                        }
                    }
                }
            }
        }
    }
    .into_string()
}

pub fn sitemap(config: &Config, posts: &[PostSummary]) -> String {
    let base = &config.base_url;
    let rfc3339 = |timestamp: &str| {
        LocalTime::from_sqlite(timestamp, config.utc_offset_minutes).map(|time| time.rfc3339())
    };
    html! {
        (XML_DECLARATION)
        urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" {
            url {
                loc { (base) "/" }
                @if let Some(date) = posts.iter().filter_map(|post| rfc3339(&post.updated_at)).max() {
                    lastmod { (date) }
                }
            }
            @for post in posts {
                url {
                    loc { (base) (post_path(&post.slug)) }
                    @if let Some(date) = rfc3339(&post.updated_at) {
                        lastmod { (date) }
                    }
                }
            }
        }
    }
    .into_string()
}
