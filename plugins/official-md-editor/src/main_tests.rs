use super::*;
use std::fs;

#[test]
fn creates_and_scans_standalone_markdown_docs() {
    let dir = std::env::temp_dir().join(format!("tomoyo_mde_test_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let path = create_md_file(&dir, "Architecture Design").unwrap();
    assert!(path.is_file());
    let docs = scan_markdown_docs(&dir);
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].title, "Architecture Design");
    assert_eq!(parse_create_title("+ 新建文档"), Some("新建文档".into()));
    let _ = fs::remove_dir_all(&dir);
}
