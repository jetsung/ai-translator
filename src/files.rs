use anyhow::Result;
use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

/// 收集所有需要翻译的文件（支持多种格式）
pub fn collect_files(root_dir: &Path, exclude_dirs: &[String], whitelist_extensions: &[String], blacklist_extensions: &[String]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    let walkdir = WalkDir::new(root_dir).follow_links(false);
    let iter = walkdir.into_iter();

    for entry in iter.filter_entry(|e| !is_excluded_dir(e, root_dir, exclude_dirs)) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };

        // Only process files, not directories
        if entry.file_type().is_file() && is_supported_format(entry.path(), whitelist_extensions, blacklist_extensions) {
            files.push(entry.path().to_path_buf());
        }
    }

    Ok(files)
}

/// Check if a directory should be excluded
fn is_excluded_dir(entry: &DirEntry, root_dir: &Path, exclude_dirs: &[String]) -> bool {
    if !entry.file_type().is_dir() {
        return false;
    }

    // Check if the directory name is in the exclusion list
    let dir_name = entry.file_name().to_string_lossy();
    if exclude_dirs.contains(&dir_name.to_string()) {
        return true;
    }

    // Check if the relative path from root contains any excluded directory name
    if let Ok(rel_path) = entry.path().strip_prefix(root_dir) {
        for exclude_dir in exclude_dirs {
            if rel_path.to_string_lossy().contains(exclude_dir) {
                return true;
            }
        }
    }

    false
}

/// 获取文件格式
pub fn get_file_format(file_path: &Path) -> Option<String> {
    if let Some(ext) = file_path.extension() {
        let ext_lower = ext.to_string_lossy().to_lowercase();
        match ext_lower.as_str() {
            "md" | "mdx" | "txt" | "srt" => return Some(ext_lower),
            _ => {}
        }
    }
    None
}

/// 检查文件是否为支持的格式
pub fn is_supported_format(file_path: &Path, whitelist_extensions: &[String], blacklist_extensions: &[String]) -> bool {
    if let Some(ext) = file_path.extension() {
        let ext_lower = ext.to_string_lossy().to_lowercase();
        
        // 如果有白名单，只接受白名单中的扩展名（忽略黑名单）
        if !whitelist_extensions.is_empty() {
            return whitelist_extensions.contains(&ext_lower);
        }
        
        // 如果在黑名单中，直接拒绝
        if !blacklist_extensions.is_empty() && blacklist_extensions.contains(&ext_lower) {
            return false;
        }
        
        // 没有白名单时，使用默认支持的格式
        match ext_lower.as_str() {
            "md" | "mdx" | "txt" | "srt" => true,
            _ => false,
        }
    } else {
        false
    }
}

/// 检测文件是否看起来像文件列表
/// 规则：读取前 n 行，如果超过 threshold 比例的非空行是 URL 或存在的本地文件，则认为是列表
pub fn is_file_list(file_path: &Path, check_lines: usize, threshold: f64) -> Result<bool, std::io::Error> {
    let file = std::fs::File::open(file_path)?;
    let reader = std::io::BufReader::new(file);
    use std::io::BufRead;
    
    let mut total_lines = 0;
    let mut valid_paths = 0;
    
    // 获取文件所在的目录，用于检查相对路径
    let base_dir = file_path.parent();
    
    for line in reader.lines().take(check_lines) {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        
        total_lines += 1;
        
        // 简单的 URL 检查
        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            valid_paths += 1;
        } else {
            // 检查是否为存在的路径
            let path = Path::new(trimmed);
            if path.exists() {
                valid_paths += 1;
            } else if let Some(base) = base_dir {
                // 尝试检查相对于文件所在目录的路径
                if base.join(path).exists() {
                    valid_paths += 1;
                }
            }
        }
    }
    
    if total_lines == 0 {
        return Ok(false);
    }
    
    Ok((valid_paths as f64 / total_lines as f64) > threshold)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_get_file_format() {
        assert_eq!(
            get_file_format(Path::new("test.md")),
            Some("md".to_string())
        );
        assert_eq!(
            get_file_format(Path::new("test.mdx")),
            Some("mdx".to_string())
        );
        assert_eq!(
            get_file_format(Path::new("test.txt")),
            Some("txt".to_string())
        );
        assert_eq!(
            get_file_format(Path::new("test.srt")),
            Some("srt".to_string())
        );
        assert_eq!(get_file_format(Path::new("test.pdf")), None);
        assert_eq!(get_file_format(Path::new("test")), None);
    }

    #[test]
    fn test_is_supported_format() {
        // 测试默认支持的格式（无白名单/黑名单）
        assert!(is_supported_format(Path::new("test.md"), &[], &[]));
        assert!(is_supported_format(Path::new("test.mdx"), &[], &[]));
        assert!(is_supported_format(Path::new("test.txt"), &[], &[]));
        assert!(is_supported_format(Path::new("test.srt"), &[], &[]));
        assert!(!is_supported_format(Path::new("test.pdf"), &[], &[]));
        assert!(!is_supported_format(Path::new("test"), &[], &[]));
        
        // 测试白名单
        let whitelist = vec!["md".to_string(), "txt".to_string()];
        assert!(is_supported_format(Path::new("test.md"), &whitelist, &[]));
        assert!(is_supported_format(Path::new("test.txt"), &whitelist, &[]));
        assert!(!is_supported_format(Path::new("test.srt"), &whitelist, &[]));
        
        // 测试黑名单
        let blacklist = vec!["txt".to_string()];
        assert!(is_supported_format(Path::new("test.md"), &[], &blacklist));
        assert!(!is_supported_format(Path::new("test.txt"), &[], &blacklist));
        
        // 测试白名单优先级（有白名单时忽略黑名单）
        let whitelist = vec!["txt".to_string()];
        let blacklist = vec!["txt".to_string()];
        assert!(is_supported_format(Path::new("test.txt"), &whitelist, &blacklist));
    }

    #[test]
    fn test_collect_files() {
        // Create a temporary directory structure
        let temp_dir = TempDir::new().unwrap();
        let root_dir = temp_dir.path();

        // Create test files
        let md_file = root_dir.join("test.md");
        let txt_file = root_dir.join("test.txt");
        let srt_file = root_dir.join("test.srt");
        let unsupported_file = root_dir.join("test.pdf");

        fs::create_dir_all(root_dir.join("subdir")).unwrap();
        let sub_md_file = root_dir.join("subdir").join("nested.md");

        fs::write(&md_file, "# Test").unwrap();
        fs::write(&txt_file, "Test content").unwrap();
        fs::write(&srt_file, "1\n00:00:01,000 --> 00:00:04,000\nTest subtitle").unwrap();
        fs::write(&unsupported_file, "PDF content").unwrap();
        fs::write(&sub_md_file, "# Nested test").unwrap();

        // Collect files
        let files = collect_files(root_dir, &[], &[], &[]).unwrap();

        assert!(files.contains(&md_file));
        assert!(files.contains(&txt_file));
        assert!(files.contains(&srt_file));
        assert!(files.contains(&sub_md_file));
        assert!(!files.contains(&unsupported_file));
        assert_eq!(files.len(), 4);
    }

    #[test]
    fn test_collect_files_with_exclusions() {
        let temp_dir = TempDir::new().unwrap();
        let root_dir = temp_dir.path();

        // Create test files with excluded directory
        let exclude_dir = root_dir.join("node_modules");
        fs::create_dir_all(&exclude_dir).unwrap();

        let normal_file = root_dir.join("test.md");
        let excluded_file = exclude_dir.join("excluded.md");

        fs::write(&normal_file, "# Test").unwrap();
        fs::write(&excluded_file, "# Excluded").unwrap();

        let files = collect_files(root_dir, &["node_modules".to_string()], &[], &[]).unwrap();

        assert!(files.contains(&normal_file));
        assert!(!files.contains(&excluded_file));
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn test_is_file_list() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let root = temp_dir.path();
        
        let target_file = root.join("target.md");
        fs::write(&target_file, "content").unwrap();
        
        let list_file = root.join("list.txt");
        // 使用相对路径编写内容
        fs::write(&list_file, "target.md\nhttps://google.com").unwrap();
        
        // 应该能识别，因为 target.md 相对于 list.txt 是存在的
        assert!(is_file_list(&list_file, 5, 0.8).unwrap());
        
        let normal_txt = root.join("normal.txt");
        fs::write(&normal_txt, "This is just some regular text.\nNot a list at all.").unwrap();
        assert!(!is_file_list(&normal_txt, 5, 0.8).unwrap());
    }
}
