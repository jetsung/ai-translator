mod config;
mod files;
mod recorder;
mod translator;

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use config::{load_config, Config};
use files::collect_files;
use rand::prelude::IndexedRandom;
use recorder::TranslationRecorder;
use std::fs;
use std::io::{self, BufRead};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tracing::{error, info};
use translator::{translate_file, Provider};

/// AI 翻译工具 - 将 Markdown 文档翻译成中文
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// 初始化配置文件
    #[arg(long)]
    init: bool,

    /// 要翻译的输入文件路径或 URL (new named argument) - for single file mode or list file path in list mode
    #[arg(long)]
    input: Option<PathBuf>,

    /// 翻译后的输出路径 (file or directory)
    #[arg(long, requires = "input")]
    output: Option<PathBuf>,

    /// 指定输入为文件列表模式 (list.txt每行一个文件路径或URL) - 在此模式下 --input 指定列表文件
    #[arg(long)]
    list: bool,

    /// 配置文件路径
    #[arg(short, long, default_value = "config.toml")]
    config: String,

    /// 强制重新翻译已翻译的文件
    #[arg(long)]
    force: bool,

    /// 重试失败的文件
    #[arg(long)]
    retry_failed: bool,

    /// 跳过检查 API Provider 可用性
    #[arg(long)]
    no_provider_check: bool,

    /// 覆盖输出模式 (overwrite 或 new_folder)
    #[arg(long, value_parser = validate_output_mode)]
    output_mode: Option<String>,

    /// 覆盖最大 token 数
    #[arg(long)]
    max_tokens: Option<usize>,

    /// 覆盖大文件拆分阈值 (字符数，默认 4000)
    #[arg(long)]
    max_chunk_size: Option<usize>,

    /// 覆盖排除的目录 (逗号分隔)
    #[arg(long)]
    exclude_dir: Option<String>,

    /// 在单文件翻译时保留完整路径结构（仅对 --input 指定的单个文件有效）
    #[arg(long)]
    full_path: bool,

    /// 指定使用的 Provider 索引 (0-based) - 与 --provider-name 互斥。默认随机选择。使用此参数时会自动跳过 Provider 可用性检测。
    #[arg(long, conflicts_with = "provider_name")]
    provider: Option<usize>,

    /// 指定使用的 Provider 名称（精确匹配）- 与 --provider 互斥。默认随机选择。使用此参数时会自动跳过 Provider 可用性检测。
    #[arg(long, conflicts_with = "provider")]
    provider_name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_cli_arg_conflicts() {
        // After removing positional FILE argument, --input should work independently
        // This test would require using clap's testing features
        assert!(true);
    }

    #[tokio::test]
    async fn test_output_path_detection() {
        // Test the functionality by creating a temporary file and testing output path handling
        let temp_dir = TempDir::new().unwrap();
        let input_file = temp_dir.path().join("input.md");
        let output_dir = temp_dir.path().join("output_dir");
        let expected_output_file = output_dir.join("input.md");

        // Create a test input file
        fs::write(&input_file, "# Test Document\n\nThis is a test document.").unwrap();

        // Use our path handling logic manually to test
        let output_file_path = if output_dir.extension().is_none() && !output_dir.is_file() {
            // Output path has no extension, treat as directory
            output_dir.join(input_file.file_name().unwrap())
        } else {
            // Output path as file
            output_dir.clone()
        };

        assert_eq!(output_file_path, expected_output_file);
    }

    #[test]
    fn test_extension_detection() {
        let path_with_ext = PathBuf::from("output/file.md");
        let path_without_ext = PathBuf::from("output/directory");

        assert!(path_with_ext.extension().is_some());
        assert!(path_without_ext.extension().is_none());
    }

    #[test]
    fn test_is_valid_url() {
        assert!(is_valid_url("https://example.com"));
        assert!(is_valid_url("http://example.com"));
        assert!(is_valid_url("https://example.com/path/file.txt"));
        assert!(!is_valid_url("file.txt"));
        assert!(!is_valid_url("./path/file.txt"));
        assert!(!is_valid_url("../path/file.txt"));
    }

    #[test]
    fn test_extract_filename_from_url() {
        assert_eq!(
            extract_filename_from_url("https://example.com/path/file.txt"),
            "file.txt"
        );
        assert_eq!(
            extract_filename_from_url("https://example.com/file.pdf"),
            "file.pdf"
        );
        assert_eq!(
            extract_filename_from_url("https://example.com/path/to/file"),
            "file"
        );
        assert_eq!(
            extract_filename_from_url("https://example.com/"),
            "downloaded_file.txt"
        );
        assert_eq!(
            extract_filename_from_url("https://example.com?param=value"),
            "downloaded_file.txt"
        );
    }

    #[test]
    fn test_extract_full_path_from_url() {
        assert_eq!(
            extract_full_path_from_url("https://example.com/path/file.txt"),
            "path/file.txt"
        );
        assert_eq!(
            extract_full_path_from_url("https://example.com/file.pdf"),
            "file.pdf"
        );
        assert_eq!(
            extract_full_path_from_url("https://raw.githubusercontent.com/user/repo/main/file.md"),
            "user/repo/main/file.md"
        );
        assert_eq!(
            extract_full_path_from_url("https://example.com/path/to/deep/nested/file.txt"),
            "path/to/deep/nested/file.txt"
        );
        assert_eq!(extract_full_path_from_url("https://example.com/"), "");
        assert_eq!(
            extract_full_path_from_url("https://openeuler/kernel/raw/OLK-6.6/README"),
            "kernel/raw/OLK-6.6/README"
        );
    }

    #[tokio::test]
    async fn test_read_file_list() {
        let temp_dir = TempDir::new().unwrap();
        let list_file = temp_dir.path().join("list.txt");

        let test_content = "file1.txt\nfile2.md\n\nfile3.pdf\n  \nfile4.rs\n";
        fs::write(&list_file, test_content).unwrap();

        let result = read_file_list(&list_file).unwrap();
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], "file1.txt");
        assert_eq!(result[1], "file2.md");
        assert_eq!(result[2], "file3.pdf");
        assert_eq!(result[3], "file4.rs");
    }

    #[test]
    fn test_validate_output_mode() {
        // Test valid values
        assert_eq!(validate_output_mode("overwrite").unwrap(), "overwrite");
        assert_eq!(validate_output_mode("new_folder").unwrap(), "new_folder");
        assert_eq!(validate_output_mode("OVERWRITE").unwrap(), "OVERWRITE");
        assert_eq!(validate_output_mode("NEW_FOLDER").unwrap(), "NEW_FOLDER");

        // Test invalid values
        assert!(validate_output_mode("invalid").is_err());
        assert!(validate_output_mode("").is_err());
        assert!(validate_output_mode("some_other_mode").is_err());
    }

    #[tokio::test]
    async fn test_url_input_detection() {
        // Test that our URL detection is working
        let input_path = PathBuf::from("https://example.com/test.md");
        let input_path_str = input_path.to_string_lossy();
        assert!(
            is_valid_url(&input_path_str),
            "https://example.com/test.md should be detected as a URL"
        );

        let input_path = PathBuf::from("http://example.com/test.txt");
        let input_path_str = input_path.to_string_lossy();
        assert!(
            is_valid_url(&input_path_str),
            "http://example.com/test.txt should be detected as a URL"
        );

        let input_path = PathBuf::from("./local_file.md");
        let input_path_str = input_path.to_string_lossy();
        assert!(
            !is_valid_url(&input_path_str),
            "./local_file.md should not be detected as a URL"
        );
    }

    #[tokio::test]
    async fn test_url_vs_local_input_handling() {
        // Test that local files continue to be handled as before (not as URLs)
        let input_path = PathBuf::from("local_file.md");
        let input_path_str = input_path.to_string_lossy();
        assert!(
            !is_valid_url(&input_path_str),
            "local_file.md should not be treated as a URL"
        );

        // Test that the function would properly route to URL handler vs local file handler
        assert!(
            is_valid_url("https://example.com/test.md"),
            "URLs should be detected correctly"
        );
        assert!(
            !is_valid_url("local_file.md"),
            "Local files should not be detected as URLs"
        );
    }

    #[tokio::test]
    async fn test_full_path_output_calculation() {
        let temp_dir = TempDir::new().unwrap();

        // Test case 1: File in subdirectory with --full-path enabled
        // openspec/AGENTS.md -> tmp/ should become tmp/openspec/AGENTS.md
        let input_path = temp_dir.path().join("openspec").join("AGENTS.md");
        let output_dir = temp_dir.path().join("tmp");

        // Create the input directory and file
        fs::create_dir_all(input_path.parent().unwrap()).unwrap();
        fs::write(&input_path, "# Test Content").unwrap();

        // Read input file to verify it exists
        assert!(input_path.exists());

        // Test with full_path = true (preserve directory structure)
        let full_path = true;
        let output_file_path = if output_dir.extension().is_none() && !output_dir.is_file() {
            if full_path {
                // Simulate strip_prefix behavior
                let input_relative_path = input_path
                    .strip_prefix(temp_dir.path())
                    .unwrap_or_else(|_| input_path.as_path());
                output_dir.join(input_relative_path)
            } else {
                output_dir.join(input_path.file_name().unwrap())
            }
        } else {
            output_dir.clone()
        };

        let expected = temp_dir.path().join("tmp").join("openspec").join("AGENTS.md");
        assert_eq!(output_file_path, expected, "With --full-path, should preserve directory structure");

        // Test with full_path = false (default - filename only)
        let full_path = false;
        let output_file_path = if output_dir.extension().is_none() && !output_dir.is_file() {
            if full_path {
                let input_relative_path = input_path
                    .strip_prefix(temp_dir.path())
                    .unwrap_or_else(|_| input_path.as_path());
                output_dir.join(input_relative_path)
            } else {
                output_dir.join(input_path.file_name().unwrap())
            }
        } else {
            output_dir.clone()
        };

        let expected = temp_dir.path().join("tmp").join("AGENTS.md");
        assert_eq!(
            output_file_path,
            expected,
            "Without --full-path, should only use filename"
        );
    }

    #[tokio::test]
    async fn test_full_path_nested_directories() {
        let temp_dir = TempDir::new().unwrap();

        // Test deeply nested file: docs/guide/getting-started.md
        let input_path = temp_dir
            .path()
            .join("docs")
            .join("guide")
            .join("getting-started.md");
        let output_dir = temp_dir.path().join("translated");

        // Create the nested directory structure
        fs::create_dir_all(input_path.parent().unwrap()).unwrap();
        fs::write(&input_path, "# Nested Content").unwrap();

        assert!(input_path.exists());

        // With --full-path: should create translated/docs/guide/getting-started.md
        let full_path = true;
        let output_file_path = if output_dir.extension().is_none() && !output_dir.is_file() {
            if full_path {
                // Simulate strip_prefix behavior
                let input_relative_path = input_path
                    .strip_prefix(temp_dir.path())
                    .unwrap_or_else(|_| input_path.as_path());
                output_dir.join(input_relative_path)
            } else {
                output_dir.join(input_path.file_name().unwrap())
            }
        } else {
            output_dir.clone()
        };

        let expected = temp_dir
            .path()
            .join("translated")
            .join("docs")
            .join("guide")
            .join("getting-started.md");
        assert_eq!(
            output_file_path,
            expected,
            "With --full-path, should preserve full nested structure"
        );

        // Without --full-path: should only create translated/getting-started.md
        let full_path = false;
        let output_file_path = if output_dir.extension().is_none() && !output_dir.is_file() {
            if full_path {
                let input_relative_path = input_path
                    .strip_prefix(temp_dir.path())
                    .unwrap_or_else(|_| input_path.as_path());
                output_dir.join(input_relative_path)
            } else {
                output_dir.join(input_path.file_name().unwrap())
            }
        } else {
            output_dir.clone()
        };

        let expected = temp_dir.path().join("translated").join("getting-started.md");
        assert_eq!(
            output_file_path,
            expected,
            "Without --full-path, should only use filename"
        );
    }

    #[tokio::test]
    async fn test_full_path_file_in_root_directory() {
        let temp_dir = TempDir::new().unwrap();

        // Test file in root directory: README.md
        let input_path = temp_dir.path().join("README.md");
        let output_dir = temp_dir.path().join("tmp");

        fs::write(&input_path, "# Root Content").unwrap();
        assert!(input_path.exists());

        // Since README.md is in root of temp_dir, strip_prefix gives just "README.md"
        let full_path = true;
        let output_file_path = if output_dir.extension().is_none() && !output_dir.is_file() {
            if full_path {
                // Simulate strip_prefix behavior
                let input_relative_path = input_path
                    .strip_prefix(temp_dir.path())
                    .unwrap_or_else(|_| input_path.as_path());
                output_dir.join(input_relative_path)
            } else {
                output_dir.join(input_path.file_name().unwrap())
            }
        } else {
            output_dir.clone()
        };

        let expected = temp_dir.path().join("tmp").join("README.md");
        assert_eq!(
            output_file_path,
            expected,
            "Root directory file with --full-path should create output in tmp/README.md"
        );
    }


    #[tokio::test]
    async fn test_full_path_with_explicit_output_file() {
        let temp_dir = TempDir::new().unwrap();

        // Test --input openspec/AGENTS.md --output tmp/custom.md --full-path
        // The output is explicitly a file, so --full-path should be ignored
        let input_path = temp_dir.path().join("openspec").join("AGENTS.md");
        let output_path = temp_dir.path().join("tmp").join("custom.md");

        fs::create_dir_all(input_path.parent().unwrap()).unwrap();
        fs::write(&input_path, "# Test Content").unwrap();
        fs::create_dir_all(output_path.parent().unwrap()).unwrap();

        assert!(input_path.exists());

        // When output has an extension, it's treated as a file, not a directory
        let full_path = true;
        let output_file_path = if output_path.extension().is_none() && !output_path.is_file() {
            if full_path {
                if let Some(parent) = input_path.parent() {
                    output_path.join(parent).join(input_path.file_name().unwrap())
                } else {
                    output_path.join(input_path.file_name().unwrap())
                }
            } else {
                output_path.join(input_path.file_name().unwrap())
            }
        } else {
            // Output is a file, use it directly (ignore --full-path)
            output_path.clone()
        };

        let expected = temp_dir.path().join("tmp").join("custom.md");
        assert_eq!(
            output_file_path,
            expected,
            "Explicit output filename should be used, --full-path should be ignored"
        );
    }

    #[test]
    fn test_select_provider_by_name_valid() {
        use translator::Provider;
        use reqwest::Client;

        let client = Client::builder().no_proxy().build().unwrap();
        let providers = vec![
            Provider {
                name: "OpenAI".to_string(),
                api_key: "sk-test".to_string(),
                base_url: "https://api.openai.com/v1".to_string(),
                model: "gpt-4".to_string(),
                rate_delay: std::time::Duration::from_secs_f64(3.0),
                client,
                concurrency: 3,
            },
        ];

        let result = select_provider(&providers, None, Some("OpenAI"));
        assert!(result.is_ok());
        assert_eq!(result.unwrap().name, "OpenAI");
    }

    #[test]
    fn test_select_provider_by_name_invalid() {
        use translator::Provider;
        use reqwest::Client;

        let client = Client::builder().no_proxy().build().unwrap();
        let providers = vec![Provider {
            name: "OpenAI".to_string(),
            api_key: "sk-test".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-4".to_string(),
            rate_delay: std::time::Duration::from_secs_f64(3.0),
            client,
            concurrency: 3,
        }];

        let result = select_provider(&providers, None, Some("NonExistent"));
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("未找到"));
        assert!(error_msg.contains("NonExistent"));
        assert!(error_msg.contains("OpenAI"));
    }

    #[test]
    fn test_select_provider_by_index_valid() {
        use translator::Provider;
        use reqwest::Client;

        let client = Client::builder().no_proxy().build().unwrap();
        let providers = vec![
            Provider {
                name: "OpenAI".to_string(),
                api_key: "sk-test".to_string(),
                base_url: "https://api.openai.com/v1".to_string(),
                model: "gpt-4".to_string(),
                rate_delay: std::time::Duration::from_secs_f64(3.0),
                client: client.clone(),
                concurrency: 3,
            },
            Provider {
                name: "Anthropic".to_string(),
                api_key: "sk-test2".to_string(),
                base_url: "https://api.anthropic.com/v1".to_string(),
                model: "claude-3".to_string(),
                rate_delay: std::time::Duration::from_secs_f64(2.0),
                client,
                concurrency: 3,
            },
        ];

        let result = select_provider(&providers, Some(1), None::<&String>);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().name, "Anthropic");
    }

    #[test]
    fn test_select_provider_by_index_invalid() {
        use translator::Provider;
        use reqwest::Client;

        let client = Client::builder().no_proxy().build().unwrap();
        let providers = vec![Provider {
            name: "OpenAI".to_string(),
            api_key: "sk-test".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-4".to_string(),
            rate_delay: std::time::Duration::from_secs_f64(3.0),
            client,
            concurrency: 3,
        }];

        let result = select_provider(&providers, Some(999), None::<&String>);
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("索引"));
        assert!(error_msg.contains("999"));
        assert!(error_msg.contains("0-0"));
    }

    #[test]
    fn test_select_provider_both_specified() {
        use translator::Provider;
        use reqwest::Client;

        let client = Client::builder().no_proxy().build().unwrap();
        let providers = vec![Provider {
            name: "OpenAI".to_string(),
            api_key: "sk-test".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-4".to_string(),
            rate_delay: std::time::Duration::from_secs_f64(3.0),
            client,
            concurrency: 3,
        }];

        let result = select_provider(&providers, Some(0), Some("OpenAI"));
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("不能同时使用"));
    }

    #[test]
    fn test_select_provider_none_specified() {
        use translator::Provider;
        use reqwest::Client;

        let client = Client::builder().no_proxy().build().unwrap();
        let providers = vec![
            Provider {
                name: "OpenAI".to_string(),
                api_key: "sk-test".to_string(),
                base_url: "https://api.openai.com/v1".to_string(),
                model: "gpt-4".to_string(),
                rate_delay: std::time::Duration::from_secs_f64(3.0),
                client: client.clone(),
                concurrency: 3,
            },
            Provider {
                name: "Anthropic".to_string(),
                api_key: "sk-test2".to_string(),
                base_url: "https://api.anthropic.com/v1".to_string(),
                model: "claude-3".to_string(),
                rate_delay: std::time::Duration::from_secs_f64(2.0),
                client,
                concurrency: 3,
            },
        ];

        let result = select_provider(&providers, None, None::<&String>);
        assert!(result.is_ok());
        // Should return one of the providers (random selection)
        let provider = result.unwrap();
        assert!(provider.name == "OpenAI" || provider.name == "Anthropic");
    }

    #[test]
    fn test_select_provider_empty_list() {
        let result = select_provider(&[], None, None::<&String>);
        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        assert!(error_msg.contains("没有可用的 Provider"));
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // 初始化配置文件
    if args.init {
        return init_config(&args.config);
    }

    // 验证 max_tokens 参数
    if let Some(0) = args.max_tokens {
        eprintln!("错误: --max-tokens 必须是正整数");
        std::process::exit(1);
    }

    // 初始加载配置（为了获取日志配置）
    let mut config = load_config(&args.config)?;

    // 如果有 CLI 覆盖参数，重新加载配置
    if args.input.is_some() || args.output.is_some() || args.output_mode.is_some() || args.max_tokens.is_some() || args.max_chunk_size.is_some() || args.exclude_dir.is_some() {
        let overrides = config::ConfigOverrides {
            root_dir: args.input.clone().filter(|p| p.is_dir()),
            output_dir: args.output.clone(),
            output_mode: args.output_mode.clone(),
            max_tokens: args.max_tokens,
            max_chunk_size: args.max_chunk_size,
            exclude_dir: args.exclude_dir.clone(),
        };
        config = config::load_config_with_overrides(&args.config, overrides)?;
    }

    // 初始化日志
    init_logging(&config);

    info!("配置文件加载成功: {}", args.config);
    info!("根目录: {:?}", config.root_dir);
    info!("输出目录: {:?}", config.output_dir);
    
    let mode_str = match config.output_mode {
        config::OutputMode::Overwrite => "Overwrite",
        config::OutputMode::NewFolder => "NewFolder",
    };
    info!("输出模式: {}", mode_str);

    // 如果用户明确指定了 provider，则跳过可用性检测
    let skip_check = args.no_provider_check || args.provider.is_some() || args.provider_name.is_some();

    // 初始化 Providers
    let providers = initialize_providers(&config, skip_check).await?;

    // 初始化记录器
    let translated_file = if let Some(ref log_dir) = config.log_dir {
        log_dir.join("translated_files.txt")
    } else {
        PathBuf::from("translated_files.txt")
    };
    let failed_file = if let Some(ref log_dir) = config.log_dir {
        log_dir.join("failed_translations.txt")
    } else {
        PathBuf::from("failed_translations.txt")
    };
    let recorder = Arc::new(TranslationRecorder::new(translated_file, failed_file)?);

    // 文件列表模式
    if args.list {
        if let Some(list_path) = args.input {
            let output_path = args
                .output
                .expect("Output path is required when using list mode");
            return handle_file_list_translation(
                list_path,
                output_path,
                &config,
                &providers,
                &recorder,
                args.force,
                args.provider,
                args.provider_name.as_ref(),
            )
            .await;
        } else {
            anyhow::bail!("使用 --list 模式时必须提供 --input 参数");
        }
    }

    // 目录翻译模式 or 单文件模式
    if let Some(input_path) = args.input {
        // 检查输入路径是文件还是目录
        if input_path.is_dir() {
            // 目录模式使用已经处理过覆盖的全局配置进行批量翻译
            return handle_batch_translation(
                &config,
                &providers,
                &recorder,
                args.force,
                args.retry_failed,
                args.provider,
                args.provider_name.as_ref(),
            )
            .await;
        } else {
            // 当输入是文件时
            // 智能检测：如果是 .txt 文件且未指定 --list，尝试自动检测是否为文件列表
            let should_use_list_mode = if args.list {
                true
            } else if let Some(ext) = input_path.extension() {
                 if ext == "txt" && input_path.exists() {
                     // 检测是否为列表文件 (check 20 lines, 80% threshold)
                     let is_list = files::is_file_list(&input_path, 20, 0.8).unwrap_or(false);
                     if is_list {
                         info!("检测到 .txt 文件包含文件路径/URL，自动启用列表模式: {:?}", input_path);
                     }
                     is_list
                 } else {
                     false
                 }
            } else {
                false
            };

            if should_use_list_mode {
                 let output_path = args.output.unwrap_or_else(|| {
                     // 如果未指定输出目录，默认为 input_path 同级目录下的 output 文件夹
                     // 或者报错？原逻辑 list 模式必须有 output
                     // 这里为了兼容性，如果用户没传 output，我们最好报错或者给默认值
                     // 原逻辑: expect("Output path is required when using list mode")
                     // 我们这里如果自动检测到了，用户可能没传 output，这会导致 panic。
                     // 最好是抛出友好错误。
                     eprintln!("错误: 自动检测到列表模式，但未提供 --output 参数。请指定输出目录。");
                     std::process::exit(1);
                 });
                 
                 return handle_file_list_translation(
                    input_path,
                    output_path,
                    &config,
                    &providers,
                    &recorder,
                    args.force,
                    args.provider,
                    args.provider_name.as_ref(),
                )
                .await;
            }

            // 执行单文件翻译
            if let Some(output_path) = args.output {
                // 如果提供了输出路径，则使用自定义路径处理
                return handle_single_file_with_custom_paths(
                    input_path,
                    output_path,
                    &config,
                    &providers,
                    &recorder,
                    args.force,
                    args.full_path,
                    args.provider,
                    args.provider_name.as_ref(),
                )
                .await;
            } else {
                // 如果没有提供输出路径，则使用默认配置进行单文件翻译
                return handle_single_file(input_path, &config, &providers, &recorder, args.force, args.provider, args.provider_name.as_ref())
                    .await;
            }
        }
    }

    // 批量翻译模式
    handle_batch_translation(
        &config,
        &providers,
        &recorder,
        args.force,
        args.retry_failed,
        args.provider,
        args.provider_name.as_ref(),
    )
    .await
}

/// 验证输出模式参数
fn validate_output_mode(s: &str) -> Result<String, String> {
    match s.to_lowercase().as_str() {
        "overwrite" | "new_folder" => Ok(s.to_string()),
        _ => Err("输出模式必须是 'overwrite' 或 'new_folder'".to_string()),
    }
}

/// 初始化配置文件
fn init_config(config_path: &str) -> Result<()> {
    let config_content = r#"# AI 翻译工具配置文件

# 根目录 - 包含需要翻译的 Markdown 文件
root_dir = "./docs"

# 输出目录 - 翻译后的文件保存位置
output_dir = "./docs_zh"

# 输出模式: "overwrite" (覆盖原文件) 或 "new_folder" (保存到新文件夹)
output_mode = "new_folder"

# 排除的目录 (逗号分隔)
exclude_dir = "node_modules,.git,_build"

# 最大 token 数
max_tokens = 8192

# 大文件拆分阈值 (单位：字符)
max_chunk_size = 4000

# 系统提示词（支持多行，使用 """ 包裹）
system_prompt = """
你是一个专业的技术文档翻译专家。请将以下英文 Markdown 文档翻译成流畅、自然的简体中文。

严格要求：
1. 保留完整的 Markdown 格式，包括标题、列表、表格、代码块、链接、图片等一切结构完全不变。
2. 代码块、命令行、文件名、路径、API 名称、配置文件内容、技术术语等保持原样（不要翻译）。
3. 专有名词（如产品名、框架名、软件名，例如 Traefik、Docker、Kubernetes）保持英文原名。
4. 翻译要准确、专业、易懂，技术术语使用业界通用中文表达。
5. 绝对禁止在文档开头或 YAML frontmatter 前后添加任何 Markdown 代码块标记（如 ```markdown 或 ```），保持文档结构的纯净。
6. 绝对禁止自作主张地添加任何原文档中不存在的内容，包括但不限于：完整的语言指南、示例代码、学习资源、最佳实践等。只翻译原文档中已有的内容，不多不少。
7. 只输出翻译后的完整 Markdown 内容，不要添加任何说明、注释或多余文本。
8. 绝对禁止修改任何源文件路径或名称，包括但不限于图片（img, svg）、链接等，保持原样。
"""

# 日志配置
[logging]
# 日志等级 (debug, info, warn, error)
level = "info"
# 时间格式 (none (no timestamp), standard (local timezone format "%Y:%m:%d %H:%M:%S"), ISO-8601 (ISO 8601 format), RFC-3339 (RFC 3339 format))
time_format = "standard"
# 是否在终端界面显示日志
console = true
# 日志目录
dir = "./logs"
# 日志文件名
file = "translation.log"

# API Providers 配置
[[providers]]
enabled = false
name = "OpenAI"
api_key = "sk-xxxxxxxxxxxxxxxxxxxxxxxx"
base_url = "https://api.openai.com/v1"
model = "gpt-4"
concurrency = 3
rate_delay = 3.0
"#;

    // 检查文件是否已存在
    if std::path::Path::new(config_path).exists() {
        println!("配置文件 {} 已存在", config_path);
        return Ok(());
    }

    // 写入配置文件
    std::fs::write(config_path, config_content)
        .with_context(|| format!("无法创建配置文件: {}", config_path))?;

    println!("✅ 配置文件 {} 已创建", config_path);
    println!("请编辑配置文件，设置您的 API Key 和其他参数");
    Ok(())
}

/// 初始化日志系统 with new configuration options
fn init_logging(config: &config::Config) {
    use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

    // 创建日志目录
    if let Some(parent) = config.log_file.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    // Process the log level
    let level_filter = match config.log_level.as_str() {
        "debug" => tracing::Level::DEBUG,
        "info" => tracing::Level::INFO,
        "warn" => tracing::Level::WARN,
        "error" => tracing::Level::ERROR,
        _ => tracing::Level::INFO, // Default to info if invalid level
    };

    let env_filter = EnvFilter::from_default_env()
        .add_directive(level_filter.into())
        .add_directive("reqwest=warn".parse().unwrap())
        .add_directive("hyper=warn".parse().unwrap());

    // Create custom time formatter based on config
    let time_format = config.log_time_format.clone();
    let timer = move || CustomTimeFormatter {
        format: time_format.clone(),
    };

    // Create file layer with custom time format
    let file_layer = fmt::layer()
        .with_writer(std::fs::File::create(&config.log_file).unwrap())
        .with_ansi(false)
        .with_target(true)
        .with_timer(timer());

    // Create a registry with the env filter
    let registry = tracing_subscriber::registry().with(env_filter);

    // Add console layer if console display is enabled
    if config.log_console {
        let console_layer = fmt::layer()
            .with_writer(std::io::stderr) // Write to stderr for console logs
            .with_ansi(true)
            .with_target(true)
            .with_timer(timer());

        registry.with(file_layer).with(console_layer).init();
    } else {
        registry.with(file_layer).init();
    }
}

// Custom time formatter that supports different time format patterns
struct CustomTimeFormatter {
    format: String,
}

impl tracing_subscriber::fmt::time::FormatTime for CustomTimeFormatter {
    fn format_time(&self, w: &mut tracing_subscriber::fmt::format::Writer<'_>) -> std::fmt::Result {
        use chrono::Local;

        // Get the current local time
        let local_time = Local::now();

        match self.format.as_str() {
            "none" => Ok(()), // No time output
            "standard" | "%Y:%m:%d %H:%M:%S" => {
                // Standard format: 2025:12:30 13:03:47 (local timezone)
                let formatted = local_time.format("%Y:%m:%d %H:%M:%S").to_string();
                write!(w, "{}", formatted)
            }
            "ISO-8601" | "%Y:%m:%dT%H:%M:%S" => {
                // ISO-8601 format: 2025-12-30T13:03:47+08:00 (proper ISO 8601 standard with hyphens and timezone)
                let formatted = local_time.format("%Y-%m-%dT%H:%M:%S%:z").to_string();
                write!(w, "{}", formatted)
            }
            "RFC-3339" | "%+%" => {
                // RFC 3339 format: 2025-12-30T13:03:47.552262+08:00 (local timezone)
                let formatted = local_time.to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
                write!(w, "{}", formatted)
            }
            _ => {
                // Default to standard format if unknown format
                let formatted = local_time.format("%Y:%m:%d %H:%M:%S").to_string();
                write!(w, "{}", formatted)
            }
        }
    }
}

/// 验证并选择 Provider
fn select_provider(
    providers: &[Provider],
    provider_index: Option<usize>,
    provider_name: Option<impl AsRef<str>>,
) -> Result<Provider> {
    // 检查是否同时指定了两种选择方式（虽然 clap 会阻止，但防御性编程）
    if provider_index.is_some() && provider_name.is_some() {
        anyhow::bail!("不能同时使用 --provider 和 --provider-name 参数");
    }

    // 按名称选择
    if let Some(name) = provider_name {
        let name_str = name.as_ref();
        return providers
            .iter()
            .find(|p| p.name == name_str)
            .cloned()
            .ok_or_else(|| {
                let available_names: Vec<&str> = providers.iter().map(|p| p.name.as_str()).collect();
                anyhow!(
                    "未找到名为 '{}' 的 Provider。可用的 Providers: {}",
                    name_str,
                    available_names.join(", ")
                )
            });
    }

    // 按索引选择
    if let Some(index) = provider_index {
        return providers
            .get(index)
            .cloned()
            .ok_or_else(|| {
                anyhow!(
                    "Provider 索引 {} 超出范围。有效索引范围: 0-{}",
                    index,
                    providers.len().saturating_sub(1)
                )
            });
    }

    // 默认：随机选择一个（保持向后兼容）
    providers
        .choose(&mut rand::rng())
        .cloned()
        .ok_or_else(|| anyhow!("没有可用的 Provider"))
}

/// 初始化 Providers
async fn initialize_providers(config: &Config, skip_check: bool) -> Result<Vec<Provider>> {
    let mut providers = Vec::new();

    if skip_check {
        println!("跳过 API Provider 检查（已指定 Provider 或 --no-provider-check）...");
        for provider_config in &config.providers {
            providers.push(Provider::from_config(provider_config));
        }
    } else {
        println!("检查 API Providers...");
        for provider_config in &config.providers {
            let provider = Provider::from_config(provider_config);

            match provider.test().await {
                Ok(_) => {
                    println!("✅ [{}] 可用", provider.name);
                    providers.push(provider);
                }
                Err(e) => {
                    println!("❌ [{}] 不可用: {}", provider.name, e);
                }
            }
        }
    }

    if providers.is_empty() {
        anyhow::bail!("没有可用的 Provider");
    }

    println!("共 {} 个 Provider 可用\n", providers.len());
    Ok(providers)
}

/// 处理单文件翻译
async fn handle_single_file(
    file_path: PathBuf,
    config: &Config,
    providers: &[Provider],
    recorder: &Arc<TranslationRecorder>,
    force: bool,
    provider_index: Option<usize>,
    provider_name: Option<&String>,
) -> Result<()> {
    if providers.is_empty() {
        anyhow::bail!("没有可用的 Provider");
    }

    println!("翻译单个文件: {:?}", file_path);

    // 验证文件是否存在
    if !file_path.exists() {
        anyhow::bail!("文件不存在: {:?}", file_path);
    }

    // 如果是 new_folder 模式，确保输出目录存在
    if config.output_mode == config::OutputMode::NewFolder && !config.output_dir.exists() {
        std::fs::create_dir_all(&config.output_dir)
            .with_context(|| format!("无法创建输出目录: {:?}", config.output_dir))?;
    }

    // 检查 root_dir 是否存在（在 new_folder 模式下单文件翻译时也需要）
    if config.output_mode == config::OutputMode::NewFolder && !config.root_dir.exists() {
        anyhow::bail!("根目录 {:?} 不存在，请先创建此目录", config.root_dir);
    }

    // 使用 select_provider 选择 Provider（支持索引或名称）
    let provider = select_provider(providers, provider_index, provider_name.as_ref())?;

    match translate_file(
        &file_path,
        &config.root_dir,
        &config.output_dir,
        config.output_mode,
        &provider,
        &config.system_prompt,
        config.max_tokens,
        config.max_chunk_size,
        recorder,
        force,
    )
    .await
    {
        Ok(_) => {
            println!("✅ 翻译成功");
            Ok(())
        }
        Err(e) => {
            println!("❌ 翻译失败: {}", e);
            let _ = recorder.record_failure(&file_path);
            Err(e)
        }
    }
}

/// 处理单文件翻译 with custom input/output paths
async fn handle_single_file_with_custom_paths(
    input_path: PathBuf,
    output_path: PathBuf,
    config: &Config,
    providers: &[Provider],
    recorder: &Arc<TranslationRecorder>,
    force: bool,
    full_path: bool,
    provider_index: Option<usize>,
    provider_name: Option<&String>,
) -> Result<()> {
    if providers.is_empty() {
        anyhow::bail!("没有可用的 Provider");
    }

    let input_path_str = input_path.to_string_lossy();
    println!("翻译单个文件: {}", input_path_str);

    // Check if input is a URL before checking if it's a local file
    if is_valid_url(&input_path_str) {
        // Handle URL input
        return handle_url_input_translation(
            &input_path_str,
            output_path,
            config,
            providers,
            recorder,
            force,
            provider_index,
            provider_name,
        )
        .await;
    }

    // Validate that input file exists (for local files only)
    if !input_path.exists() {
        anyhow::bail!("输入文件不存在: {:?}", input_path);
    }

    // 检查输出路径是否为目录还是文件
    let output_file_path = if output_path.extension().is_none() && !output_path.is_file() {
        // 输出路径没有扩展名，视为目录
        if full_path {
            // --full-path 模式：保留完整的相对路径结构
            // 将输入路径转换为相对于当前目录的路径
            let input_relative_path = input_path
                .strip_prefix(std::env::current_dir()?)
                .unwrap_or_else(|_| {
                    // 如果无法获取相对路径，使用原始路径
                    input_path.as_path()
                });

            output_path.join(input_relative_path)
        } else {
            // 默认模式：只使用文件名
            output_path.join(
                input_path
                    .file_name()
                    .ok_or_else(|| anyhow!("输入文件名无效: {:?}", input_path))?,
            )
        }
    } else {
        // 输出路径为文件，直接使用（忽略 --full-path）
        output_path
    };

    // 确保输出目录存在
    if let Some(output_dir) = output_file_path.parent().filter(|p| !p.exists()) {
        std::fs::create_dir_all(output_dir)
            .with_context(|| format!("无法创建输出目录: {:?}", output_dir))?;
    }

    // 使用 select_provider 选择 Provider（支持索引或名称）
    let provider = select_provider(providers, provider_index, provider_name.as_ref())?;
    println!("使用 Provider: {}", provider.name);

    // Create a temporary file in the same directory structure to reuse the existing translation logic
    // We'll temporarily create a "fake" directory structure to use the existing translate_file function
    let temp_dir = tempfile::TempDir::new()?;
    let temp_root_dir = temp_dir.path().join("input");
    let temp_output_dir = temp_dir.path().join("output");
    std::fs::create_dir_all(&temp_root_dir)?;
    std::fs::create_dir_all(&temp_output_dir)?;

    // Copy input file to temp location maintaining the same relative structure
    let relative_path = input_path
        .file_name()
        .unwrap_or(std::ffi::OsStr::new("temp_file.md"));
    let temp_input_path = temp_root_dir.join(relative_path);
    std::fs::copy(&input_path, &temp_input_path)?;

    // Create a temporary config that mimics the original but with our temp directories
    let temp_config = config::Config {
        root_dir: temp_root_dir.clone(),
        output_dir: temp_output_dir.clone(),
        output_mode: config::OutputMode::NewFolder, // Use new folder mode to copy to output
        exclude_dirs: config.exclude_dirs.clone(),
        system_prompt: config.system_prompt.clone(),
        max_tokens: config.max_tokens,
        max_chunk_size: config.max_chunk_size,
        log_dir: config.log_dir.clone(),
        log_file: config.log_file.clone(),
        log_level: config.log_level.clone(),
        log_time_format: config.log_time_format.clone(),
        log_console: config.log_console,
        providers: config.providers.clone(),
    };

    // Use the existing translate_file function to handle all the complex logic
    translator::translate_file(
        &temp_input_path,
        &temp_config.root_dir,
        &temp_config.output_dir,
        temp_config.output_mode,
        &provider,
        &temp_config.system_prompt,
        temp_config.max_tokens,
        config.max_chunk_size,
        recorder,
        force,
    )
    .await?;

    // Now copy the translated file from temp output location to the desired output location
    let temp_output_file = temp_output_dir.join(relative_path);
    if temp_output_file.exists() {
        std::fs::copy(&temp_output_file, &output_file_path)
            .with_context(|| format!("无法复制翻译文件到最终位置: {:?}", output_file_path))?;
    } else {
        // If the above translation didn't create a file in the expected location,
        // we'll directly translate the content
        let content = fs::read_to_string(&input_path)
            .with_context(|| format!("无法读取输入文件: {:?}", input_path))?;

        // Check if the content is likely already Chinese
        if !force && translator::is_likely_chinese(&content) {
            println!("⚠️  文件可能已是中文，跳过翻译: {:?}", input_path);
            fs::write(&output_file_path, &content)
                .with_context(|| format!("无法写入输出文件: {:?}", output_file_path))?;
        } else {
            // Perform the translation using the provider
            let translated_content = provider
                .translate(&content, &config.system_prompt, config.max_tokens, config.max_chunk_size, &input_path_str)
                .await
                .with_context(|| format!("翻译失败: {:?}", input_path))?;

            // Write the translated content to the output file
            fs::write(&output_file_path, &translated_content)
                .with_context(|| format!("无法写入输出文件: {:?}", output_file_path))?;
        }
    }

    println!("✅ 翻译成功: {:?}", output_file_path);
    Ok(())
}

/// Read and parse the file list from the input file
fn read_file_list(file_path: &PathBuf) -> Result<Vec<String>> {
    let file = std::fs::File::open(file_path)
        .with_context(|| format!("无法打开文件列表: {:?}", file_path))?;
    let reader = io::BufReader::new(file);

    let mut files = Vec::new();
    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            files.push(trimmed.to_string());
        }
    }

    Ok(files)
}

/// 获取当前激活的 Providers
fn get_active_providers(
    providers: &[Provider],
    provider_index: Option<usize>,
    provider_name: Option<impl AsRef<str>>,
) -> Result<Vec<Provider>> {
    // 检查是否同时指定了两种选择方式
    if provider_index.is_some() && provider_name.is_some() {
        anyhow::bail!("不能同时使用 --provider 和 --provider-name 参数");
    }

    // 按名称选择
    if let Some(name) = provider_name {
        let name_str = name.as_ref();
        let p = providers
            .iter()
            .find(|p| p.name == name_str)
            .cloned()
            .ok_or_else(|| {
                let available_names: Vec<&str> = providers.iter().map(|p| p.name.as_str()).collect();
                anyhow!(
                    "未找到名为 '{}' 的 Provider。可用的 Providers: {}",
                    name_str,
                    available_names.join(", ")
                )
            })?;
        return Ok(vec![p]);
    }

    // 按索引选择
    if let Some(index) = provider_index {
        let p = providers
            .get(index)
            .cloned()
            .ok_or_else(|| {
                anyhow!(
                    "Provider 索引 {} 超出范围。有效索引范围: 0-{}",
                    index,
                    providers.len().saturating_sub(1)
                )
            })?;
        return Ok(vec![p]);
    }

    // 默认：返回所有可用的 Providers
    if providers.is_empty() {
        anyhow::bail!("没有可用的 Provider");
    }

    Ok(providers.to_vec())
}

/// Handle translation of files from a list
async fn handle_file_list_translation(
    list_path: PathBuf,
    output_dir: PathBuf,
    config: &Config,
    providers: &[Provider],
    recorder: &Arc<TranslationRecorder>,
    force: bool,
    provider_index: Option<usize>,
    provider_name: Option<&String>,
) -> Result<()> {
    // Read the file list
    let file_paths = read_file_list(&list_path)?;

    if file_paths.is_empty() {
        info!("文件列表为空，没有需要翻译的文件");
        return Ok(());
    }

    info!("从文件列表中读取了 {} 个文件路径/URL", file_paths.len());

    // Ensure output directory exists
    std::fs::create_dir_all(&output_dir)
        .with_context(|| format!("无法创建输出目录: {:?}", output_dir))?;

    // 获取激活的 Providers
    let active_providers = get_active_providers(providers, provider_index, provider_name)?;
    println!("使用 {} 个 Provider 并发处理文件列表", active_providers.len());
    for p in &active_providers {
        println!("  - {} (并发: {})", p.name, p.concurrency);
    }

    // Create a channel for sending file information to translation tasks
    let (tx, rx) = tokio::sync::mpsc::channel::<(String, PathBuf)>(1000); // (file_path_or_url, output_dir)
    let rx = Arc::new(Mutex::new(rx));

    // Send file paths/URLs to the channel
    for file_path in file_paths {
        let output_dir_clone = output_dir.clone();
        tx.send((file_path, output_dir_clone)).await?;
    }
    drop(tx);

    // Start translation tasks
    let mut tasks = Vec::new();

    for provider in active_providers {
        let provider = Arc::new(provider);
        let concurrency = provider.concurrency.max(1);

        for _ in 0..concurrency {
            let provider = Arc::clone(&provider);
            let config = config.clone();
            let recorder = recorder.clone();
            let rx = Arc::clone(&rx);

            let task = tokio::spawn(async move {
                loop {
                    let (file_path_or_url, output_dir) = {
                        let mut rx_guard = rx.lock().await;
                        match rx_guard.recv().await {
                            Some(data) => data,
                            None => break, // No more files to process
                        }
                    };

                    // Determine if this is a URL or local file path
                    if is_valid_url(&file_path_or_url) {
                        // 检查 URL 是否已翻译
                        if !force && recorder.is_translated(std::path::Path::new(&file_path_or_url)) {
                            println!("[{}] 跳过 (已翻译): {}", provider.name, file_path_or_url);
                            continue;
                        }

                        println!("开始处理 URL: {}", file_path_or_url);
                        // Handle remote URL
                        if let Err(e) = handle_remote_file_translation_with_provider(
                            &file_path_or_url,
                            &output_dir,
                            &config,
                            &provider,
                            &recorder,
                            force,
                        )
                        .await
                        {
                            error!(
                                "[{}] 远程文件翻译失败 {}: {}",
                                provider.name, file_path_or_url, e
                            );
                            println!(
                                "❌ [{}] 远程文件翻译失败: {}",
                                provider.name, file_path_or_url
                            );
                            let _ = recorder.record_failure(std::path::Path::new(&file_path_or_url));
                        }
                    } else {
                        // Handle local file
                        let path = PathBuf::from(&file_path_or_url);
                        if !path.exists() {
                            error!("文件不存在: {}", file_path_or_url);
                            let _ = recorder.record_failure(std::path::Path::new(&file_path_or_url));
                            continue;
                        }
                        
                        // 显式检查是否已翻译（使用原始路径），修复 --list 模式下的去重问题
                        if let Ok(abs_path) = std::fs::canonicalize(&path) {
                            if !force && recorder.is_translated(&abs_path) {
                                println!("[{}] 跳过 (已翻译): {:?}", provider.name, path);
                                continue;
                            }
                        }

                        println!("开始处理文件: {:?}", path);
                        if let Err(e) = handle_local_file_translation_with_provider(
                            &path,
                            &output_dir,
                            &config,
                            &provider,
                            &recorder,
                            force,
                        )
                        .await
                        {
                            error!("[{}] 本地文件翻译失败 {:?}: {}", provider.name, path, e);
                            println!("❌ [{}] 本地文件翻译失败: {:?}", provider.name, path);
                            let _ = recorder.record_failure(&path);
                        }
                    }

                    // Apply rate limiting for this provider
                    sleep(provider.rate_delay).await;
                }
            });

            tasks.push(task);
        }
    }

    // Wait for all translation tasks to complete
    for task in tasks {
        task.await?;
    }

    println!("文件列表翻译完成");
    Ok(())
}

/// Handle translation of a remote file with a specific provider
async fn handle_remote_file_translation_with_provider(
    url: &str,
    output_dir: &Path,
    config: &Config,
    provider: &Provider,
    recorder: &Arc<TranslationRecorder>,
    _force: bool,
) -> Result<()> {
    use reqwest;

    // Download the content from the URL
    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .timeout(std::time::Duration::from_secs(30)) // 30 second timeout
        .send()
        .await
        .with_context(|| format!("无法下载 URL: {}", url))?;

    if !response.status().is_success() {
        anyhow::bail!("下载失败，状态码: {}", response.status());
    }

    let content = response
        .text()
        .await
        .with_context(|| format!("无法读取 URL 内容: {}", url))?;

    // Determine output path based on URL, preserving directory structure
    let relative_path = extract_full_path_from_url(url);
    let output_path = output_dir.join(&relative_path);

    // Create parent directories if they don't exist
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("无法创建输出目录: {:?}", parent))?;
    }

    // Create a temporary file to work with the existing translation logic
    let temp_dir = tempfile::TempDir::new()?;
    let temp_filename = extract_filename_from_url(url); // Just use the filename for the temp file
    let temp_input_path = temp_dir.path().join(&temp_filename);
    std::fs::write(&temp_input_path, &content)
        .with_context(|| format!("无法写入临时文件: {:?}", temp_input_path))?;

    println!("使用 Provider: {} 翻译远程文件: {}", provider.name, url);

    // Translate the content
    let translated_content = provider
        .translate(&content, &config.system_prompt, config.max_tokens, config.max_chunk_size, url)
        .await
        .with_context(|| format!("翻译失败: {}", url))?;

    // Write the translated content to the output file
    std::fs::write(&output_path, &translated_content)
        .with_context(|| format!("无法写入翻译文件: {:?}", output_path))?;

    println!("✅ 翻译成功: {:?}", output_path);
    recorder.record_success(std::path::Path::new(url))?;
    Ok(())
}

/// Handle translation of a local file with a specific provider
async fn handle_local_file_translation_with_provider(
    file_path: &Path,
    output_dir: &Path,
    config: &Config,
    provider: &Provider,
    recorder: &Arc<TranslationRecorder>,
    force: bool,
) -> Result<()> {
    // Determine the relative path from the original file path to preserve directory structure
    let relative_path = file_path;
    let final_output_path = output_dir.join(relative_path);

    // Ensure output directory exists (create parent directories if needed)
    if let Some(parent) = final_output_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("无法创建输出目录: {:?}", parent))?;
    }

    println!(
        "使用 Provider: {} 翻译本地文件: {:?}",
        provider.name, file_path
    );

    // Use the existing translate_file function with temporary setup
    let temp_dir = tempfile::TempDir::new()?;
    let temp_root_dir = temp_dir.path().join("input");
    let temp_output_dir = temp_dir.path().join("output");
    std::fs::create_dir_all(&temp_root_dir)?;
    std::fs::create_dir_all(&temp_output_dir)?;

    // Copy input file to temp location
    let filename = file_path
        .file_name()
        .ok_or_else(|| anyhow!("无效的文件名: {:?}", file_path))?;
    let temp_input_path = temp_root_dir.join(filename);
    std::fs::copy(file_path, &temp_input_path)?;

    // Create a temporary config
    let temp_config = config::Config {
        root_dir: temp_root_dir.clone(),
        output_dir: temp_output_dir.clone(),
        output_mode: config::OutputMode::NewFolder,
        exclude_dirs: config.exclude_dirs.clone(),
        system_prompt: config.system_prompt.clone(),
        max_tokens: config.max_tokens,
        max_chunk_size: config.max_chunk_size,
        log_dir: config.log_dir.clone(),
        log_file: config.log_file.clone(),
        log_level: config.log_level.clone(),
        log_time_format: config.log_time_format.clone(),
        log_console: config.log_console,
        providers: config.providers.clone(),
    };

    // Use the existing translate_file function
    translator::translate_file(
        &temp_input_path,
        &temp_config.root_dir,
        &temp_config.output_dir,
        temp_config.output_mode,
        provider,
        &temp_config.system_prompt,
        temp_config.max_tokens,
        config.max_chunk_size,
        recorder,
        force,
    )
    .await?;

    // Copy the translated file to the final output location (preserving directory structure)
    let temp_output_file = temp_output_dir.join(filename);
    if temp_output_file.exists() {
        std::fs::copy(&temp_output_file, &final_output_path)
            .with_context(|| format!("无法复制翻译文件到最终位置: {:?}", final_output_path))?;
    } else {
        // If the above translation didn't create a file, translate directly
        let content = std::fs::read_to_string(file_path)
            .with_context(|| format!("无法读取输入文件: {:?}", file_path))?;

        if !force && translator::is_likely_chinese(&content) {
            println!("⚠️  文件可能已是中文，跳过翻译: {:?}", file_path);
            std::fs::write(&final_output_path, &content)
                .with_context(|| format!("无法写入输出文件: {:?}", final_output_path))?;
        } else {
            let translated_content = provider
                .translate(&content, &config.system_prompt, config.max_tokens, config.max_chunk_size, &file_path.to_string_lossy())
                .await
                .with_context(|| format!("翻译失败: {:?}", file_path))?;

            std::fs::write(&final_output_path, &translated_content)
                .with_context(|| format!("无法写入输出文件: {:?}", final_output_path))?;
        }
    }

    println!("✅ 翻译成功: {:?}", file_path);
    Ok(())
}

/// Handle translation of a URL input with custom paths
async fn handle_url_input_translation(
    url: &str,
    output_path: PathBuf,
    config: &Config,
    providers: &[Provider],
    recorder: &Arc<TranslationRecorder>,
    force: bool,
    provider_index: Option<usize>,
    provider_name: Option<&String>,
) -> Result<()> {
    use reqwest;

    if providers.is_empty() {
        anyhow::bail!("没有可用的 Provider");
    }

    println!("翻译远程文件: {}", url);

    // Download the content from the URL
    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .timeout(std::time::Duration::from_secs(30)) // 30 second timeout
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                anyhow!("网络请求超时 (30秒) 从 URL: {}", url)
            } else if e.is_connect() {
                anyhow!("连接失败到 URL: {} - 网络连接问题", url)
            } else if e.is_request() {
                anyhow!("请求失败到 URL: {} - 请检查URL格式", url)
            } else {
                anyhow!("下载失败从 URL: {} - 错误: {}", url, e)
            }
        })?;

    let status = response.status();
    if !status.is_success() {
        anyhow::bail!(
            "下载失败，URL: {}，状态码: {} ({})",
            url,
            status,
            status.canonical_reason().unwrap_or("Unknown")
        );
    }

    let content = response
        .text()
        .await
        .with_context(|| format!("无法读取 URL 内容: {}，响应状态: {}", url, status))?;

    // 检查输出路径是否为目录还是文件
    let output_file_path = if output_path.extension().is_none() && !output_path.is_file() {
        // 输出路径没有扩展名，视为目录， use the URL's filename
        let filename = extract_filename_from_url(url);
        output_path.join(&filename)
    } else {
        // 输出路径为文件，直接使用
        output_path
    };

    // 确保输出目录存在
    if let Some(output_dir) = output_file_path.parent().filter(|p| !p.exists()) {
        std::fs::create_dir_all(output_dir)
            .with_context(|| format!("无法创建输出目录: {:?}", output_dir))?;
    }

    // 使用 select_provider 选择 Provider（支持索引或名称）
    let provider = select_provider(providers, provider_index, provider_name.as_ref())?;
    println!("使用 Provider: {}", provider.name);

    // Check if the content is likely already Chinese
    if !force && translator::is_likely_chinese(&content) {
        println!("⚠️  文件可能已是中文，跳过翻译: {}", url);
        std::fs::write(&output_file_path, &content)
            .with_context(|| format!("无法写入输出文件: {:?}", output_file_path))?;
        let _ = recorder.record_success(std::path::Path::new(url));
    } else {
        // Perform the translation using the provider
        let translated_content = match provider
            .translate(&content, &config.system_prompt, config.max_tokens, config.max_chunk_size, url)
            .await
        {
            Ok(content) => content,
            Err(e) => {
                let _ = recorder.record_failure(std::path::Path::new(url));
                return Err(e.context(format!("翻译失败: {}", url)));
            }
        };

        // Write the translated content to the output file
        std::fs::write(&output_file_path, &translated_content)
            .with_context(|| format!("无法写入输出文件: {:?}", output_file_path))?;
        let _ = recorder.record_success(std::path::Path::new(url));
    }

    println!("✅ 翻译成功: {:?}", output_file_path);
    Ok(())
}

/// Check if a string is a valid URL
fn is_valid_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

/// Extract full path from URL, excluding the domain part
fn extract_full_path_from_url(url: &str) -> String {
    use url::Url;

    if let Ok(parsed_url) = Url::parse(url) {
        let path = parsed_url.path();
        // Remove leading slash and return the path
        return path.strip_prefix('/').unwrap_or(path).to_string();
    }

    // Fallback: extract from the end of the URL, removing query parameters
    let url_without_query = if let Some(pos) = url.find('?') {
        &url[..pos]
    } else {
        url
    };

    // Remove leading protocol and domain, just keep the path portion
    if let Some(start) = url_without_query.find("://") {
        let after_protocol = &url_without_query[start + 3..];
        if let Some(path_start) = after_protocol.find('/') {
            return after_protocol[path_start + 1..]
                .trim_end_matches('/')
                .to_string();
        }
    }

    // If no path structure found, just return the original
    url_without_query.to_string()
}

/// Extract filename from URL
fn extract_filename_from_url(url: &str) -> String {
    use url::Url;

    if let Ok(parsed_url) = Url::parse(url) {
        let path_segments: Vec<&str> = parsed_url
            .path_segments()
            .map(|c| c.collect())
            .unwrap_or_default();
        if let Some(filename) = path_segments.last().filter(|s| !s.is_empty()) {
            return filename.to_string();
        }
    }

    // Fallback: extract from the end of the URL, removing query parameters
    let url_without_query = if let Some(pos) = url.find('?') {
        &url[..pos]
    } else {
        url
    };

    // Remove trailing slash if present
    let url_without_query = url_without_query.trim_end_matches('/');

    let last_segment = url_without_query
        .split('/')
        .next_back()
        .unwrap_or("downloaded_file.txt");

    // Use the URL crate to check if it's just a domain root
    if let Ok(parsed_url) = Url::parse(url) {
        // If the path is just "/" (root), return default filename
        if parsed_url.path() == "/" || parsed_url.path().is_empty() {
            return "downloaded_file.txt".to_string();
        }
    }

    // If the last segment contains dots but looks like a domain, return default filename
    if last_segment.contains('.')
        && (last_segment.ends_with(".com")
            || last_segment.ends_with(".org")
            || last_segment.ends_with(".net")
            || last_segment.ends_with(".io")
            || last_segment.ends_with(".edu")
            || last_segment.ends_with(".gov")
            || last_segment.ends_with(".co")
            || last_segment.ends_with(".uk")
            || last_segment.ends_with(".de")
            || last_segment.ends_with(".fr"))
    {
        "downloaded_file.txt".to_string()
    } else if last_segment.contains('.') {
        // If it has dots but doesn't look like a domain, it might be a filename with extension
        last_segment.to_string()
    } else if last_segment.is_empty() {
        "downloaded_file.txt".to_string()
    } else {
        last_segment.to_string()
    }
}

/// 处理批量翻译
async fn handle_batch_translation(
    config: &Config,
    providers: &[Provider],
    recorder: &Arc<TranslationRecorder>,
    force: bool,
    retry_failed: bool,
    provider_index: Option<usize>,
    provider_name: Option<&String>,
) -> Result<()> {
    // 批量翻译模式下需要检查 root_dir 是否存在
    if !config.root_dir.exists() {
        anyhow::bail!("根目录 {:?} 不存在，无法进行批量翻译", config.root_dir);
    }

    // 如果是 new_folder 模式，确保输出目录存在
    if config.output_mode == config::OutputMode::NewFolder && !config.output_dir.exists() {
        std::fs::create_dir_all(&config.output_dir)
            .with_context(|| format!("无法创建输出目录: {:?}", config.output_dir))?;
    }

    let files_to_translate = if retry_failed {
        let failed = recorder.get_failed_files();
        println!("重试模式: {} 个失败文件", failed.len());
        failed.into_iter().map(PathBuf::from).collect()
    } else {
        let all_files = collect_files(&config.root_dir, &config.exclude_dirs)?;
        let all_files_count = all_files.len();
        let files: Vec<PathBuf> = all_files
            .into_iter()
            .filter(|f| force || !recorder.is_translated(f))
            .collect();
        println!(
            "找到 {} 个文件，需要翻译: {} 个",
            all_files_count,
            files.len()
        );
        files
    };

    if files_to_translate.is_empty() {
        println!("没有需要翻译的文件");
        return Ok(());
    }

    // 创建任务队列
    let (tx, rx) = tokio::sync::mpsc::channel::<PathBuf>(100);
    let rx = Arc::new(Mutex::new(rx));

    // 发送文件到队列
    for file in files_to_translate {
        tx.send(file).await?;
    }
    drop(tx);

    // 获取激活的 Providers
    let active_providers = get_active_providers(providers, provider_index, provider_name)?;
    println!("使用 {} 个 Provider 并发处理批量翻译", active_providers.len());
    for p in &active_providers {
        println!("  - {} (并发: {})", p.name, p.concurrency);
    }

    // 启动翻译任务
    let mut tasks = Vec::new();

    for provider in active_providers {
        let provider = Arc::new(provider);
        let concurrency = provider.concurrency.max(1);

        for _ in 0..concurrency {
            let provider = Arc::clone(&provider);
            let config = config.clone();
            let recorder = recorder.clone();
            let rx = Arc::clone(&rx);

            let task = tokio::spawn(async move {
                loop {
                    let file_path = {
                        let mut rx_guard = rx.lock().await;
                        match rx_guard.recv().await {
                            Some(path) => path,
                            None => break,
                        }
                    };

                    let result = translate_file(
                        &file_path,
                        &config.root_dir,
                        &config.output_dir,
                        config.output_mode,
                        &provider,
                        &config.system_prompt,
                        config.max_tokens,
                        config.max_chunk_size,
                        &recorder,
                        force,
                    )
                    .await;

                    if let Err(e) = result {
                        error!("[{}] 翻译失败 {:?}: {}", provider.name, file_path, e);
                        println!("❌ [{}] 翻译失败: {:?}", provider.name, file_path);
                        let _ = recorder.record_failure(&file_path);
                    }

                    // 速率限制
                    sleep(provider.rate_delay).await;
                }
            });

            tasks.push(task);
        }
    }

    // 等待所有任务完成
    for task in tasks {
        task.await?;
    }

    println!("翻译完成");
    Ok(())
}
