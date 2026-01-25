# AI Translator

基于 Rust 开发的高性能 AI 文档翻译工具，旨在将技术文档（特别是 Markdown）精准地翻译成流畅的简体中文。

## 🌟 核心特性

- **🚀 工业级并发性能**：支持多 Provider（OpenAI, Claude, DeepSeek 等）并发翻译，支持抢占式任务分发。
- **🧠 智能模式识别**：自动检测 `.txt` 文件内容。若前 20 行中 80% 为有效路径或 URL，自动切换为**列表模式**。
- **✂️ 大文件智能拆分**：支持 `max_chunk_size` 配置（默认 4000 字符），自动按段落和行拆分超长文档，完美解决模型上下文限制。
- **🛡️ 列表保护机制**：在目录翻译时自动识别 `.txt` 列表文件并进行原样复制，防止误翻译导致的清单损坏。
- **🌐 全场景支持**：完美支持本地单文件、目录递归、文本列表清单以及远程 URL 直接下载并翻译。
- **🔄 断点续传与去重**：实时记录翻译状态，支持重试失败任务，智能检测内容是否已为中文以避免重复翻译。
- **🛠️ 高度可定制**：灵活的配置文件支持，命令行参数可全覆盖。

## 📦 安装

### 从源码编译

```bash
cargo build --release
```

编译后的二进制文件位于 `target/release/aitr`。

### 使用 Cargo 安装
```bash
cargo install --git https://github.com/jetsung/ai-translator.git --locked
```

## ⚙️ 快速开始

### 1. 初始化配置文件

```bash
aitr --init
```
这将在当前目录生成 `config.toml`。请在其中填入您的 API Key 和 Provider 信息。

### 2. 翻译单文件或 URL

```bash
# 本地文件
aitr --input path/to/readme.md

# 远程 URL
aitr --input https://example.com/docs/guide.md --output ./zh_docs/
```

### 3. 目录批量翻译

```bash
aitr --input ./en_docs --output ./zh_docs
```

### 4. 列表清单模式

创建一个 `list.txt`，每行一个路径或 URL，然后运行：

```bash
aitr --list --input list.txt --output ./translated
```
*注：即使不带 `--list`，程序检测到内容为列表时也会自动切换模式。*

## 🛠️ 高级用法

### 覆盖拆分阈值
针对极长文档，可以手动调整分块大小：
```bash
aitr --input long_doc.md --max-chunk-size 8000
```

### 指定特定提供商并保留并发
```bash
# 使用名为 "DeepSeek" 的 Provider 翻译，并使用该 Provider 配置的并发数
aitr --input ./docs --provider-name DeepSeek
```

### 重试失败任务
```bash
aitr --retry-failed
```

## 📖 配置文件说明 (`config.toml`)

```toml
# 基础配置
root_dir = "./docs"
output_dir = "./docs_zh"
output_mode = "new_folder"    # overwrite 或 new_folder
max_tokens = 8192
max_chunk_size = 4000         # 大文件拆分字符阈值
exclude_dir = ".git,node_modules"

# Provider 配置 (支持多个)
[[providers]]
name = "OpenAI"
enabled = true
api_key = "sk-..."
base_url = "https://api.openai.com/v1"
model = "gpt-4"
concurrency = 3               # 并发任务数
rate_delay = 1.0              # 请求间隔延迟 (秒)
```

## 命令行参数一览

| 参数 | 说明 |
|------|------|
| `--init` | 初始化配置文件 |
| `--input` | 输入路径（文件/目录/URL） |
| `--output` | 输出路径 |
| `--list` | 启用列表模式 |
| `--max-chunk-size` | 覆盖大文件拆分阈值 |
| `--max-tokens` | 覆盖单次请求最大 Token |
| `--provider-name` | 指定使用的 Provider 名称 |
| `--force` | 强制重新翻译 |
| `--retry-failed` | 仅重试之前失败的文件 |
| `--full-path` | 保留完整目录结构 |

## 🛠️ 开发

项目提供了 `Makefile` 以简化开发流程：

- `make test`: 运行所有单元测试。
- `make lint`: 运行代码风格检查。
- `make run-examples`: 顺序运行包含大文件、列表、URL 的全场景示例。
- `make clean`: 清理日志、翻译记录及示例输出。

## 许可证
Apache License 2.0

## 仓库镜像

[MyCode](https://git.jetsung.com/jetsung/ai-translator) ● [Framagit](https://framagit.org/jetsung/ai-translator) ● [GitCode](https://gitcode.com/jetsung/ai-translator) ● [GitHub](https://github.com/jetsung/ai-translator)