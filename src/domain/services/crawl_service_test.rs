// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

#[cfg(test)]
mod tests {
    use crate::domain::services::crawl_service::LinkDiscoverer;
    use std::collections::HashSet;

    // --- LinkDiscoverer Tests ---

    #[test]
    fn test_extract_links() {
        let html = r##"
            <html>
                <body>
                    <a href="https://example.com/page1">Page 1</a>
                    <a href="/page2">Page 2</a>
                    <a href="page3.html">Page 3</a>
                    <a href="#fragment">Fragment</a>
                    <a href="mailto:test@example.com">Email</a>
                    <a href="javascript:void(0)">JS</a>
                </body>
            </html>
        "##;
        let base_url = "https://example.com";

        let links = LinkDiscoverer::extract_links(html, base_url).unwrap();

        assert!(links.contains("https://example.com/page1"));
        assert!(links.contains("https://example.com/page2"));
        assert!(links.contains("https://example.com/page3.html"));
        assert_eq!(links.len(), 3);
    }

    #[test]
    fn test_filter_links() {
        let mut links = HashSet::new();
        links.insert("https://example.com/blog/1".to_string());
        links.insert("https://example.com/shop/item".to_string());
        links.insert("https://example.com/about".to_string());

        let include_patterns = vec!["blog".to_string(), "about".to_string()];
        let exclude_patterns = vec!["shop".to_string()];

        let filtered = LinkDiscoverer::filter_links(links, &include_patterns, &exclude_patterns);

        assert!(filtered.contains("https://example.com/blog/1"));
        assert!(filtered.contains("https://example.com/about"));
        assert!(!filtered.contains("https://example.com/shop/item"));
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_filter_links_no_include() {
        let mut links = HashSet::new();
        links.insert("https://example.com/blog/1".to_string());
        links.insert("https://example.com/shop/item".to_string());

        let include_patterns = vec![];
        let exclude_patterns = vec!["shop".to_string()];

        let filtered = LinkDiscoverer::filter_links(links, &include_patterns, &exclude_patterns);

        assert!(filtered.contains("https://example.com/blog/1"));
        assert!(!filtered.contains("https://example.com/shop/item"));
        assert_eq!(filtered.len(), 1);
    }
}
