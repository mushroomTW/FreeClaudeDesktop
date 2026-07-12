#[cfg(test)]
mod tests {
    use crate::core::config::Language;

    #[test]
    fn test_language_translation() {
        assert_eq!(Language::En.tr("connection_settings"), "Connection Settings");
        assert_eq!(Language::ZhTw.tr("connection_settings"), "連線設定");
        assert_eq!(Language::En.tr("unknown_key"), "unknown_key");
        assert_eq!(Language::ZhTw.tr("unknown_key"), "unknown_key");
    }
}
