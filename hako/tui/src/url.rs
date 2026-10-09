/// Utilities for detecting and validating URLs vs file paths.

/// Check if a string is a URL (starts with http:// or https://).
pub fn is_url(input: &str) -> bool {
    input.starts_with("http://") || input.starts_with("https://")
}

/// Check if a string is a file path (anything that's not a URL).
pub fn is_file_path(input: &str) -> bool {
    !is_url(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_url() {
        assert!(is_url("https://example.com/image.jpg"));
        assert!(is_url("http://example.com/image.jpg"));
        assert!(!is_url("/path/to/image.jpg"));
        assert!(!is_url("~/Pictures/image.jpg"));
        assert!(!is_url("image.jpg"));
    }

    #[test]
    fn test_is_file_path() {
        assert!(is_file_path("/path/to/image.jpg"));
        assert!(is_file_path("~/Pictures/image.jpg"));
        assert!(is_file_path("image.jpg"));
        assert!(!is_file_path("https://example.com/image.jpg"));
        assert!(!is_file_path("http://example.com/image.jpg"));
    }
}
