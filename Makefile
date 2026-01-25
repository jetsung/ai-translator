# Makefile for AI Translator

.PHONY: all test check lint fmt clean help example-dir example-list example-file example-url run-examples

# 默认目标
all: fmt lint test

# 运行所有单元测试
test:
	cargo test

# 运行特定模块的测试（例如: make test-translator）
test-%:
	cargo test $*

# 检查代码是否能编译
check:
	cargo check

# 运行 Clippy 静态检查
lint:
	cargo clippy -- -D warnings

# 格式化代码
fmt:
	cargo fmt

# 清理构建产物
clean:
	cargo clean
	rm -rf outputs
	rm -rf logs/*
	rm -f translated_files.txt failed_translations.txt

# --- 示例测试指令 (注意：需要配置真实的 config.toml) ---

# 1. 目录翻译模式 (测试智能列表识别与复制)
example-dir:
	@rm -rf outputs
	@mkdir -p outputs
	cargo run -- --input examples --output outputs --config config.toml --force

# 2. 列表模式翻译 (测试多并发、URL下载、大文件拆分)
example-list:
	@rm -rf outputs
	@mkdir -p outputs
	cargo run -- --list --input examples/list.txt --output outputs --config config.toml --force

# 3. 单本地文件翻译
example-file:
	@rm -rf outputs
	@mkdir -p outputs
	cargo run -- --input examples/normal_file.md --output outputs/normal_zh.md --config config.toml --force

# 4. 远程 URL 翻译
example-url:
	@rm -rf outputs
	@mkdir -p outputs
	cargo run -- --input https://raw.githubusercontent.com/rust-lang/rust/master/README.md --output outputs/rust_readme_zh.md --config config.toml --force

# 运行所有示例
run-examples: example-dir example-list example-file example-url

# 帮助信息
help:
	@echo "AI Translator Makefile 帮助:"
	@echo "  make test           - 运行所有单元测试"
	@echo "  make lint           - 运行 Clippy 静态检查"
	@echo "  make fmt            - 格式化代码"
	@echo "  make check          - 检查代码编译"
	@echo "  make clean          - 清理临时文件和日志"
	@echo "  make example-dir    - [示例] 目录翻译模式"
	@echo "  make example-list   - [示例] 列表模式翻译"
	@echo "  make example-file   - [示例] 单文件翻译"
	@echo "  make example-url    - [示例] 远程 URL 翻译"
	@echo "  make run-examples   - [示例] 顺序运行所有示例"
	@echo "  make test-[module]  - 运行指定模块的测试 (如: make test-config)"