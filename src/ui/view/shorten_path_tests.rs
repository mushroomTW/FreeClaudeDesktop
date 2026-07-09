use super::components::shorten_path;

#[test]
fn short_path_unchanged() {
    assert_eq!(
        shorten_path("C:\\Users\\file.txt", 60),
        "C:\\Users\\file.txt"
    );
}

#[test]
fn long_path_truncates_safely() {
    let long =
        "C:\\Users\\mushroomMaster\\Documents\\FreeClaudeDesktop\\very\\deeply\\nested\\file.txt";
    let result = shorten_path(long, 30);
    assert!(result.len() <= 33); // head + "..." + tail
    assert!(result.contains("..."));
}

#[test]
fn non_ascii_path_does_not_panic() {
    // 含中文的路徑，截斷點不能落在多位元組字元中間。
    let long = "C:\\Users\\中文字\\Desktop\\FreeClaudeDesktop\\super\\deeply\\nested\\file.txt";
    let result = shorten_path(long, 24);
    assert!(result.starts_with(|c: char| c.is_ascii()));
    assert!(result.contains("..."));
}
