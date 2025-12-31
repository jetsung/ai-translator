# AI Translator

基于 Rust 的 AI 文档翻译工具，将 Markdown 文档翻译成简体中文。支持 OpenAI 兼容接口，支持多 Provider 并发，支持本地文件、本地目录及远程 URL 翻译。

## 功能特性

- 🚀 高性能异步翻译，支持多 Provider 并发及速率限制
- 🌐 支持远程 URL 翻译及本地文件/目录批量翻译
- 📋 支持从文件列表（List Mode）批量处理多个文件或 URL
- 📝 智能处理 Markdown 格式，保留 YAML frontmatter 及特定技术术语
- 🔄 支持断点续传，记录已翻译和失败的文件，支持重试失败任务
- ⚙️ 灵活的配置文件支持，支持命令行参数覆盖
- 📊 详细的日志记录，支持自定义时间格式及控制台输出

## 安装

### 从源码编译

```bash
cargo build --release
```

编译后的二进制文件位于 `target/release/ai-translator`

### Cargo 方式
```bash
cargo install --git https://git.jetsung.com/jetsung/ai-translator.git --locked
```

## 配置

你可以使用 `--init` 命令生成默认配置文件：

```bash
./ai-translator --init
```

或者复制示例配置文件并根据需要修改：

```bash
cp config.toml.example config.toml
```

配置文件说明：

```toml
# 根目录 - 包含需要翻译的 Markdown 文件 (批量模式下使用)
root_dir = "./docs"

# 输出目录 - 翻译后的文件保存位置
output_dir = "./docs_zh"

# 输出模式: "overwrite" (覆盖原文件) 或 "new_folder" (保存到新文件夹，保持目录结构)
output_mode = "new_folder"

# 排除的目录 (逗号分隔)
exclude_dir = "node_modules,.git,_build"

# 最大 token 数
max_tokens = 8192

# 系统提示词（支持多行，使用 """ 包裹）
system_prompt = """
你是一个专业的技术文档翻译专家。请将以下英文 Markdown 文档翻译成流畅、自然的简体中文。
...
"""

# 日志配置
[logging]
# 日志等级 (debug, info, warn, error)
level = "info"
# 时间格式 (none, standard, ISO-8601, RFC-3339)
time_format = "standard"
# 是否在终端界面显示日志
console = true
# 日志目录
dir = "./logs"
# 日志文件名
file = "translation.log"

# API Providers 配置 (可以配置多个)
[[providers]]
enabled = true
name = "OpenAI"
api_key = "sk-xxxxxxxxxxxxxxxxxxxxxxxx"
base_url = "https://api.openai.com/v1"
model = "gpt-4"
concurrency = 3
rate_delay = 3.0
```

## 使用方法

### 初始化配置文件

```bash
./ai-translator --init
```

### 翻译单个文件或 URL

```bash
# 翻译本地文件
./ai-translator --input path/to/file.md

# 翻译远程 URL
./ai-translator --input https://raw.githubusercontent.com/user/repo/main/README.md --output ./downloads/README_zh.md
```

### 翻译单个文件并指定输出路径

```bash
# 指定输出文件路径
./ai-translator --input path/to/input.md --output path/to/output.md

# 指定输出目录（使用输入文件名）
./ai-translator --input path/to/input.md --output output_directory
```

### 批量翻译 (目录模式)

```bash
# 使用配置文件中的 root_dir 进行翻译
./ai-translator

# 指定输入目录和输出目录
./ai-translator --input ./my_docs --output ./my_docs_zh
```

### 文件列表模式 (List Mode)

从一个文本文件中读取多个文件路径或 URL 进行翻译（每行一个）：

```bash
./ai-translator --list --input list.txt --output ./translated_files
```

### 常用参数组合

```bash
# 强制重新翻译
./ai-translator --force

# 重试失败的文件
./ai-translator --retry-failed

# 覆盖配置文件中的输出模式
./ai-translator --output-mode overwrite

# 跳过 Provider 可用性检查
./ai-translator --no-provider-check
```

## 命令行参数

| 参数 | 说明 |
|------|------|
| `--init` | 初始化配置文件 |
| `--input` | 输入文件路径、目录或 URL |
| `--output` | 输出路径（文件或目录） |
| `--list` | 启用文件列表模式，`--input` 为列表文件路径 |
| `--config`, `-c` | 配置文件路径（默认：config.toml） |
| `--force` | 强制重新翻译已翻译的文件 |
| `--retry-failed` | 仅重试之前失败的文件 |
| `--no-provider-check` | 跳过启动时的 API Provider 可用性检查 |
| `--output-mode` | 覆盖输出模式 (`overwrite` 或 `new_folder`) |
| `--max-tokens` | 覆盖最大 Token 数 |
| `--exclude-dir` | 覆盖排除的目录（逗号分隔） |

## 翻译规则

1. **格式保留**：保留完整的 Markdown 格式（标题、列表、表格、链接、图片等）。
2. **术语保护**：代码块、命令行、文件名、路径、API 名称、技术术语保持原样。
3. **专有名词**：产品名、框架名（如 Traefik, Kubernetes）保持英文。
4. **智能过滤**：自动识别 YAML frontmatter，保留 tags、keywords、aliases 等字段。
5. **增量翻译**：默认跳过已翻译成功的本地文件。

## 开发

### 运行测试

```bash
cargo test
```

### 检查代码

```bash
cargo check
```

### 格式化代码

```bash
cargo fmt
```

## 许可证
Apache License 2.0
