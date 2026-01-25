use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// 配置覆盖参数
#[derive(Debug, Clone, Default)]
pub struct ConfigOverrides {
    pub root_dir: Option<PathBuf>,
    pub output_dir: Option<PathBuf>,
    pub output_mode: Option<String>,
    pub max_tokens: Option<usize>,
    pub max_chunk_size: Option<usize>,
    pub exclude_dir: Option<String>,
}

/// 默认系统提示词
pub const DEFAULT_SYSTEM_PROMPT: &str = r#"
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
"#;

/// Provider 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub name: Option<String>,
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_concurrency")]
    pub concurrency: usize,
    #[serde(default = "default_rate_delay")]
    pub rate_delay: f64,
}

fn default_enabled() -> bool {
    false
}

fn default_concurrency() -> usize {
    1
}

fn default_rate_delay() -> f64 {
    3.0
}

/// Logging configuration
#[derive(Debug, Deserialize, Default, PartialEq)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default = "default_time_format")]
    pub time_format: String,
    #[serde(default = "default_console")]
    pub console: bool,
    #[serde(default = "default_log_dir")]
    pub dir: Option<String>,
    #[serde(default = "default_log_file")]
    pub file: String,
}

/// TOML 配置文件结构
#[derive(Debug, Deserialize)]
pub struct TomlConfig {
    pub root_dir: String,
    pub output_dir: String,
    pub output_mode: String,
    #[serde(default)]
    pub exclude_dir: String,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: usize,
    #[serde(default = "default_max_chunk_size")]
    pub max_chunk_size: usize,
    #[serde(default = "default_log_dir")]
    pub log_dir: Option<String>,
    #[serde(default = "default_log_file")]
    pub log_file: String,
    #[serde(default)]
    pub logging: Option<LoggingConfig>,
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
}

fn default_max_tokens() -> usize {
    8192
}

fn default_max_chunk_size() -> usize {
    4000
}

fn default_log_file() -> String {
    "translation.log".to_string()
}

fn default_log_dir() -> Option<String> {
    None
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_time_format() -> String {
    "standard".to_string()
}

fn default_console() -> bool {
    true
}

/// 应用配置
#[derive(Debug, Clone)]
pub struct Config {
    pub root_dir: PathBuf,
    pub output_dir: PathBuf,
    pub output_mode: OutputMode,
    pub exclude_dirs: Vec<String>,
    pub system_prompt: String,
    pub max_tokens: usize,
    pub max_chunk_size: usize,
    pub log_dir: Option<PathBuf>,
    pub log_file: PathBuf,
    pub log_level: String,
    pub log_time_format: String,
    pub log_console: bool,
    pub providers: Vec<ProviderConfig>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    Overwrite,
    NewFolder,
}

impl OutputMode {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "overwrite" => OutputMode::Overwrite,
            "new_folder" => OutputMode::NewFolder,
            _ => OutputMode::NewFolder,
        }
    }
}

/// 加载配置文件
pub fn load_config(config_path: &str) -> Result<Config> {
    load_config_with_overrides(config_path, ConfigOverrides::default())
}

/// 加载配置文件并应用覆盖参数
pub fn load_config_with_overrides(config_path: &str, overrides: ConfigOverrides) -> Result<Config> {
    let config_content = fs::read_to_string(config_path)
        .with_context(|| format!("无法读取配置文件: {}", config_path))?;

    let toml_config: TomlConfig = toml::from_str(&config_content)
        .with_context(|| format!("解析 TOML 配置文件失败: {}", config_path))?;

    // root_dir 使用 PathBuf::from 而不是 canonicalize，因为单文件翻译模式下不需要 root_dir 存在
    let root_dir = if let Some(ref override_root_dir) = overrides.root_dir {
        override_root_dir.clone()
    } else {
        PathBuf::from(&toml_config.root_dir)
    };

    // output_dir 使用 PathBuf::from，因为主程序会处理目录创建
    let output_dir = if let Some(ref override_output_dir) = overrides.output_dir {
        override_output_dir.clone()
    } else {
        PathBuf::from(&toml_config.output_dir)
    };

    let exclude_dirs = if let Some(ref override_exclude_dir) = overrides.exclude_dir {
        override_exclude_dir
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    } else {
        toml_config
            .exclude_dir
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    };

    let system_prompt = toml_config
        .system_prompt
        .unwrap_or_else(|| DEFAULT_SYSTEM_PROMPT.to_string())
        .trim()
        .to_string();

    // Handle logging configuration - new config always takes priority when present
    let (log_dir, log_file) = if let Some(ref new_logging) = toml_config.logging {
        // Use new logging configuration when present
        let dir = new_logging.dir.as_ref().map(PathBuf::from);
        let file = if let Some(ref dir) = dir {
            dir.join(&new_logging.file)
        } else {
            PathBuf::from(&new_logging.file)
        };
        (dir, file)
    } else {
        // Fallback to old configuration only if new config is not present
        let dir = toml_config.log_dir.as_ref().map(PathBuf::from);
        let file = if let Some(ref dir) = dir {
            dir.join(&toml_config.log_file)
        } else {
            PathBuf::from(&toml_config.log_file)
        };
        (dir, file)
    };

    // 处理 providers
    let providers: Vec<ProviderConfig> = toml_config
        .providers
        .into_iter()
        .filter_map(|mut p| {
            p.api_key = p.api_key.trim().to_string();
            p.base_url = p.base_url.trim().trim_end_matches('/').to_string();
            p.model = p.model.trim().to_string();

            if !p.enabled || p.api_key.is_empty() || p.base_url.is_empty() {
                return None;
            }

            p.concurrency = p.concurrency.max(1);
            Some(p)
        })
        .collect();

    // Get logging configuration values - new config always takes priority when present
    let (log_level, log_time_format, log_console) =
        if let Some(ref new_logging) = toml_config.logging {
            // Use new logging values when new config is present
            (
                new_logging.level.clone(),
                new_logging.time_format.clone(),
                new_logging.console,
            )
        } else {
            // Use defaults when no new config is present
            (
                default_log_level(),
                default_time_format(),
                default_console(),
            )
        };

    // 应用覆盖参数
    let output_mode = if let Some(ref override_output_mode) = overrides.output_mode {
        OutputMode::from_str(override_output_mode)
    } else {
        OutputMode::from_str(&toml_config.output_mode)
    };

    let max_tokens = overrides.max_tokens.unwrap_or(toml_config.max_tokens);
    let max_chunk_size = overrides.max_chunk_size.unwrap_or(toml_config.max_chunk_size);

    Ok(Config {
        root_dir,
        output_dir,
        output_mode,
        exclude_dirs,
        system_prompt,
        max_tokens,
        max_chunk_size,
        log_dir,
        log_file,
        log_level,
        log_time_format,
        log_console,
        providers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_output_mode_from_str() {
        assert_eq!(OutputMode::from_str("overwrite"), OutputMode::Overwrite);
        assert_eq!(OutputMode::from_str("Overwrite"), OutputMode::Overwrite);
        assert_eq!(OutputMode::from_str("new_folder"), OutputMode::NewFolder);
        assert_eq!(OutputMode::from_str("New_Folder"), OutputMode::NewFolder);
        // Default case
        assert_eq!(OutputMode::from_str("invalid"), OutputMode::NewFolder);
    }

    #[test]
    fn test_default_values() {
        assert_eq!(default_max_tokens(), 8192);
        assert_eq!(default_log_file(), "translation.log");
        assert_eq!(default_enabled(), false);
        assert_eq!(default_concurrency(), 1);
        assert_eq!(default_rate_delay(), 3.0);
    }

    #[test]
    fn test_load_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test_config.toml");

        let config_content = r#"
            root_dir = "./docs"
            output_dir = "./docs_zh"
            output_mode = "new_folder"
            max_tokens = 4096
            log_file = "test.log"

            [[providers]]
            name = "TestProvider"
            api_key = "test_key"
            base_url = "https://api.openai.com/v1"
            model = "gpt-4"
            enabled = true
            concurrency = 2
            rate_delay = 2.5
        "#;

        fs::write(&config_path, config_content).unwrap();

        let result = load_config(config_path.to_str().unwrap());
        assert!(result.is_ok());

        let config = result.unwrap();
        assert_eq!(config.max_tokens, 4096);
        assert_eq!(config.log_file, PathBuf::from("test.log"));
        assert_eq!(config.providers.len(), 1);
        assert_eq!(config.providers[0].model, "gpt-4");
        assert_eq!(config.providers[0].concurrency, 2);
    }

    #[test]
    fn test_load_config_with_log_dir() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test_config_with_log_dir.toml");

        let config_content = r#"
            root_dir = "./docs"
            output_dir = "./docs_zh"
            output_mode = "new_folder"
            max_tokens = 4096
            log_dir = "./logs"
            log_file = "test_with_dir.log"

            [[providers]]
            name = "TestProvider"
            api_key = "test_key"
            base_url = "https://api.openai.com/v1"
            model = "gpt-4"
            enabled = true
            concurrency = 2
            rate_delay = 2.5
        "#;

        fs::write(&config_path, config_content).unwrap();

        let result = load_config(config_path.to_str().unwrap());
        assert!(result.is_ok());

        let config = result.unwrap();
        assert_eq!(config.max_tokens, 4096);
        assert_eq!(config.log_dir, Some(PathBuf::from("./logs")));
        assert_eq!(config.log_file, PathBuf::from("./logs/test_with_dir.log"));
        assert_eq!(config.providers.len(), 1);
        assert_eq!(config.providers[0].model, "gpt-4");
        assert_eq!(config.providers[0].concurrency, 2);
    }

    #[test]
    fn test_load_config_without_log_dir() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test_config_without_log_dir.toml");

        let config_content = r#"
            root_dir = "./docs"
            output_dir = "./docs_zh"
            output_mode = "new_folder"
            max_tokens = 4096
            log_file = "test_without_dir.log"

            [[providers]]
            name = "TestProvider"
            api_key = "test_key"
            base_url = "https://api.openai.com/v1"
            model = "gpt-4"
            enabled = true
            concurrency = 2
            rate_delay = 2.5
        "#;

        fs::write(&config_path, config_content).unwrap();

        let result = load_config(config_path.to_str().unwrap());
        assert!(result.is_ok());

        let config = result.unwrap();
        assert_eq!(config.max_tokens, 4096);
        assert_eq!(config.log_dir, None);
        assert_eq!(config.log_file, PathBuf::from("test_without_dir.log"));
        assert_eq!(config.providers.len(), 1);
        assert_eq!(config.providers[0].model, "gpt-4");
        assert_eq!(config.providers[0].concurrency, 2);
    }

    #[test]
    fn test_load_config_with_new_logging_section() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir
            .path()
            .join("test_config_with_logging_section.toml");

        let config_content = r#"
            root_dir = "./docs"
            output_dir = "./docs_zh"
            output_mode = "new_folder"
            max_tokens = 4096

            [logging]
            level = "debug"
            time_format = "none"
            console = false
            dir = "./logs"
            file = "new_test.log"

            [[providers]]
            name = "TestProvider"
            api_key = "test_key"
            base_url = "https://api.openai.com/v1"
            model = "gpt-4"
            enabled = true
            concurrency = 2
            rate_delay = 2.5
        "#;

        fs::write(&config_path, config_content).unwrap();

        let result = load_config(config_path.to_str().unwrap());
        assert!(result.is_ok());

        let config = result.unwrap();
        assert_eq!(config.max_tokens, 4096);
        assert_eq!(config.log_level, "debug");
        assert_eq!(config.log_time_format, "none");
        assert_eq!(config.log_console, false);
        assert_eq!(config.log_dir, Some(PathBuf::from("./logs")));
        assert_eq!(config.log_file, PathBuf::from("./logs/new_test.log"));
        assert_eq!(config.providers.len(), 1);
        assert_eq!(config.providers[0].model, "gpt-4");
        assert_eq!(config.providers[0].concurrency, 2);
    }

    #[test]
    fn test_load_config_with_old_logging_section_priority() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test_config_old_overrides.toml");

        let config_content = r#"
            root_dir = "./docs"
            output_dir = "./docs_zh"
            output_mode = "new_folder"
            max_tokens = 4096
            log_dir = "./old_logs"
            log_file = "old_test.log"

            [logging]
            level = "debug"
            time_format = "none"
            console = false
            dir = "./logs"
            file = "new_test.log"

            [[providers]]
            name = "TestProvider"
            api_key = "test_key"
            base_url = "https://api.openai.com/v1"
            model = "gpt-4"
            enabled = true
            concurrency = 2
            rate_delay = 2.5
        "#;

        fs::write(&config_path, config_content).unwrap();

        let result = load_config(config_path.to_str().unwrap());
        assert!(result.is_ok());

        let config = result.unwrap();
        // New logging config should take priority
        assert_eq!(config.log_dir, Some(PathBuf::from("./logs")));
        assert_eq!(config.log_file, PathBuf::from("./logs/new_test.log"));
        assert_eq!(config.log_level, "debug");
        assert_eq!(config.log_time_format, "none");
        assert_eq!(config.log_console, false);
    }

    #[test]
    fn test_load_config_with_empty_new_logging_section() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test_config_empty_logging.toml");

        let config_content = r#"
            root_dir = "./docs"
            output_dir = "./docs_zh"
            output_mode = "new_folder"
            max_tokens = 4096
            log_dir = "./old_logs"
            log_file = "old_test.log"

            [logging]

            [[providers]]
            name = "TestProvider"
            api_key = "test_key"
            base_url = "https://api.openai.com/v1"
            model = "gpt-4"
            enabled = true
            concurrency = 2
            rate_delay = 2.5
        "#;

        fs::write(&config_path, config_content).unwrap();

        let result = load_config(config_path.to_str().unwrap());
        assert!(result.is_ok());

        let config = result.unwrap();
        // With new behavior, when [logging] section is present (even if empty),
        // it uses the new config with default values
        assert_eq!(config.log_dir, None); // Default dir value is None
        assert_eq!(config.log_file, PathBuf::from("translation.log")); // Default file name
        assert_eq!(config.log_level, "info");
        assert_eq!(config.log_time_format, "standard");
        assert_eq!(config.log_console, true);
    }

    #[test]
    fn test_load_config_with_overrides() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test_config_with_overrides.toml");

        let config_content = r#"
            root_dir = "./docs"
            output_dir = "./docs_zh"
            output_mode = "new_folder"
            max_tokens = 4096
            exclude_dir = "node_modules,.git"

            [[providers]]
            name = "TestProvider"
            api_key = "test_key"
            base_url = "https://api.openai.com/v1"
            model = "gpt-4"
            enabled = true
            concurrency = 2
            rate_delay = 2.5
        "#;

        fs::write(&config_path, config_content).unwrap();

        // Test with overrides
        let overrides = ConfigOverrides {
            root_dir: Some(PathBuf::from("./override_docs")),
            output_dir: Some(PathBuf::from("./override_output")),
            output_mode: Some("overwrite".to_string()),
            max_tokens: Some(2048),
            max_chunk_size: None,
            exclude_dir: Some("temp,build".to_string()),
        };

        let result = load_config_with_overrides(config_path.to_str().unwrap(), overrides);
        assert!(result.is_ok());

        let config = result.unwrap();
        assert_eq!(config.root_dir, PathBuf::from("./override_docs"));
        assert_eq!(config.output_dir, PathBuf::from("./override_output"));
        assert_eq!(config.output_mode, OutputMode::Overwrite);
        assert_eq!(config.max_tokens, 2048);
        assert_eq!(
            config.exclude_dirs,
            vec!["temp".to_string(), "build".to_string()]
        );
    }

    #[test]
    fn test_load_config_with_partial_overrides() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test_config_partial_overrides.toml");

        let config_content = r#"
            root_dir = "./docs"
            output_dir = "./docs_zh"
            output_mode = "new_folder"
            max_tokens = 4096
            exclude_dir = "node_modules,.git"

            [[providers]]
            name = "TestProvider"
            api_key = "test_key"
            base_url = "https://api.openai.com/v1"
            model = "gpt-4"
            enabled = true
            concurrency = 2
            rate_delay = 2.5
        "#;

        fs::write(&config_path, config_content).unwrap();

        // Test with partial overrides - only output_mode is overridden
        let overrides = ConfigOverrides {
            root_dir: None,
            output_dir: None,
            output_mode: Some("overwrite".to_string()),
            max_tokens: None,
            max_chunk_size: None,
            exclude_dir: None,
        };

        let result = load_config_with_overrides(config_path.to_str().unwrap(), overrides);
        assert!(result.is_ok());

        let config = result.unwrap();
        assert_eq!(config.root_dir, PathBuf::from("./docs")); // Original value
        assert_eq!(config.output_dir, PathBuf::from("./docs_zh")); // Original value
        assert_eq!(config.output_mode, OutputMode::Overwrite); // Overridden value
        assert_eq!(config.max_tokens, 4096); // Original value
        assert_eq!(
            config.exclude_dirs,
            vec!["node_modules".to_string(), ".git".to_string()]
        ); // Original value
    }

    #[test]
    fn test_load_config_with_overrides_preserves_providers() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test_config_preserve_providers.toml");

        let config_content = r#"
            root_dir = "./docs"
            output_dir = "./docs_zh"
            output_mode = "new_folder"
            max_tokens = 4096
            exclude_dir = "node_modules"

            [[providers]]
            name = "TestProvider"
            api_key = "test_key"
            base_url = "https://api.openai.com/v1"
            model = "gpt-4"
            enabled = true
            concurrency = 2
            rate_delay = 2.5
        "#;

        fs::write(&config_path, config_content).unwrap();

        let overrides = ConfigOverrides {
            root_dir: Some(PathBuf::from("./new_docs")),
            output_dir: None,
            output_mode: None,
            max_tokens: None,
            max_chunk_size: None,
            exclude_dir: None,
        };

        let result = load_config_with_overrides(config_path.to_str().unwrap(), overrides);
        assert!(result.is_ok());

        let config = result.unwrap();
        assert_eq!(config.root_dir, PathBuf::from("./new_docs"));
        assert_eq!(config.providers.len(), 1);
        assert_eq!(config.providers[0].model, "gpt-4");
        assert_eq!(config.providers[0].concurrency, 2);
    }
}
