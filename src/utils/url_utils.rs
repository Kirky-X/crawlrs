// Copyright 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use url::{ParseError, Url};

/// 将可能为相对路径的URL转换为绝对路径URL
pub fn resolve_url(base_url: &Url, path: &str) -> Result<Url, ParseError> {
    base_url.join(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_absolute_url() {
        let base = Url::parse("http://example.com/a/b").unwrap();
        let path = "http://t.co/c";
        assert_eq!(resolve_url(&base, path).unwrap().as_str(), "http://t.co/c");
    }

    #[test]
    fn test_resolve_protocol_relative_url() {
        let base = Url::parse("https://example.com/a/b").unwrap();
        let path = "//t.co/c";
        assert_eq!(resolve_url(&base, path).unwrap().as_str(), "https://t.co/c");
    }

    #[test]
    fn test_resolve_root_relative_url() {
        let base = Url::parse("http://example.com/a/b").unwrap();
        let path = "/c";
        assert_eq!(
            resolve_url(&base, path).unwrap().as_str(),
            "http://example.com/c"
        );
    }

    #[test]
    fn test_resolve_relative_url() {
        let base = Url::parse("http://example.com/a/b").unwrap();
        let path = "c";
        assert_eq!(
            resolve_url(&base, path).unwrap().as_str(),
            "http://example.com/a/c"
        );
    }
}
