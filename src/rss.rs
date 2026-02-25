use chrono::NaiveDate;

use crate::handler::render_markdown;
use crate::template::DirEntry;

/// Build a complete RSS 2.0 feed as an XML string.
///
/// `base_url` should be an absolute origin like `"https://example.com"` (no
/// trailing slash). When empty, item links are relative paths and
/// `<guid isPermaLink>` is set to `"false"`.
pub fn build_feed(
    channel_title: &str,
    channel_link: &str,
    channel_description: &str,
    items: &[DirEntry],
    base_url: &str,
) -> String {
    // Determine whether we can emit valid absolute URLs.
    let has_absolute_base = base_url.starts_with("http://") || base_url.starts_with("https://");
    let permalink_attr = if has_absolute_base { "true" } else { "false" };

    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <rss version=\"2.0\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n\
         <channel>\n",
    );

    xml.push_str(&format!("  <title>{}</title>\n", esc(channel_title)));
    xml.push_str(&format!("  <link>{}</link>\n", esc(channel_link)));
    xml.push_str(&format!(
        "  <description>{}</description>\n",
        esc(channel_description)
    ));

    for item in items {
        let title = item.title.as_deref().unwrap_or(&item.display_name);
        let link = format!("{}{}", base_url.trim_end_matches('/'), item.url);

        xml.push_str("  <item>\n");
        xml.push_str(&format!("    <title>{}</title>\n", esc(title)));
        xml.push_str(&format!("    <link>{}</link>\n", esc(&link)));
        xml.push_str(&format!(
            "    <guid isPermaLink=\"{}\">{}</guid>\n",
            permalink_attr,
            esc(&link)
        ));
        if let Some(md) = &item.content {
            let html = render_markdown(md);
            xml.push_str("    <description><![CDATA[");
            xml.push_str(&html);
            xml.push_str("]]></description>\n");
        } else if let Some(summary) = &item.summary {
            xml.push_str(&format!(
                "    <description>{}</description>\n",
                esc(summary)
            ));
        }
        if let Some(date) = &item.date {
            xml.push_str(&format!("    <pubDate>{}</pubDate>\n", to_rfc822(date)));
        }
        if let Some(author) = &item.author {
            // <dc:creator> accepts a plain name; the core RSS <author> element
            // requires an email address, which we don't have.
            xml.push_str(&format!("    <dc:creator>{}</dc:creator>\n", esc(author)));
        }
        xml.push_str("  </item>\n");
    }

    xml.push_str("</channel>\n</rss>");
    xml
}

/// XML-escape a string for use in element content or attribute values.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Convert a `YYYY-MM-DD` date string to RFC 822 format required by RSS 2.0.
/// Falls back to the original string if parsing fails.
fn to_rfc822(date_str: &str) -> String {
    NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
        .map(|d| d.format("%a, %d %b %Y 00:00:00 +0000").to_string())
        .unwrap_or_else(|_| date_str.to_string())
}
